//! JavaScript source compiled to VM bytecode and executed by tasks
//! (clj-quickjs-22). Programs here are JavaScript source: synthetic fixtures
//! (`tests/fixtures/js/`, not light-cljs output) and inline snippets. The
//! bytecode-corruption tests start from compiled code and edit its bytes.

use clj_quickjs::frontend::{
    ScriptOptions, SyntaxErrorKind, compile_function, compile_function_with, compile_script,
    compile_script_with,
};
use clj_quickjs::{
    Budget, Cancel, Completion, DependencyId, DependencyKey, Executable, Host, HostCall, Opcode,
    Primitive, ResultKind, RunOutcome, Runtime, ValidationErrorKind, ValueView, VmError,
    VmErrorKind,
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
    let roots: Vec<_> = args.iter().map(|p| rt.root(p.clone())).collect();
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
            | Opcode::GetCapture
            | Opcode::GetField
            | Opcode::PutField
            | Opcode::DefineField
            | Opcode::GetBuiltinGlobal
            | Opcode::CallMethod
            | Opcode::Construct => 2,
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
        // `+` with a binding operand may concatenate strings: `plus`.
        Opcode::Plus,
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

/// Loose equality with a boolean on the left (clj-quickjs-60). The first
/// four rows are the `null`/`undefined`/boolean cases of `test_eq` (line
/// 126) of `research/quickjs/tests/test_language.js`, revision 7d1c13c5d8f8
/// (each `assert(cond)` as a Rust assertion on the expression's value; the
/// string cases are in `strings_compare_loosely_with_numbers` and the
/// wrapper-object cases wait for ToPrimitive);
/// see `docs/architecture.md`, "QuickJS references and provenance".
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
fn loose_equality_with_a_boolean_left_operand() {
    for (src, expected) in [
        // `test_eq`.
        ("null == undefined", true),
        ("undefined == null", true),
        ("true == 1", true),
        ("0 == false", true),
        // A boolean on the left compares as a number.
        ("false == 0", true),
        ("false == -0", true),
        ("true == 2", false),
        ("true != 1", false),
        ("false != 0", false),
        ("true == NaN", false),
        // Booleans against booleans, `null` and `undefined`.
        ("true == true", true),
        ("true == false", false),
        ("false == null", false),
        ("false == undefined", false),
    ] {
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
    // A compound assignment's value is the stored value (clj-quickjs-84).
    assert_eq!(num("let i = 0; while (i < 3) { i += 2; }"), 4.0);
    assert_eq!(num("let a = 6; 1; a /= 4;"), 1.5);
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
    // Through a finally block (clj-quickjs-79): a normally completing
    // finally block never sets the value; a jump out of a finally block
    // carries the value that block produced.
    assert_eq!(
        num("5; while (true) { try { 6; break; } finally { 7; } }"),
        6.0
    );
    assert_eq!(
        num("while (true) { try { throw 1; } finally { 8; break; } }"),
        8.0
    );
    assert_eq!(
        eval("while (true) { try { 6; break; } finally { break; } }"),
        ValueView::Undefined
    );
    assert_eq!(
        eval("let i = 0; while (i < 2) { i = i + 1; try { 6; } finally { continue; } }"),
        ValueView::Undefined
    );
    // `do`-`while` (clj-quickjs-80): reset to undefined on entry, then the
    // value the body produced last (kept across iterations); the test's
    // value is never the completion value.
    assert_eq!(num("5; do { 6; } while (false)"), 6.0);
    assert_eq!(eval("5; do { } while (false)"), ValueView::Undefined);
    assert_eq!(eval("5; do ; while (false)"), ValueView::Undefined);
    // `for`-`in` (clj-quickjs-85): reset to undefined, then UpdateEmpty.
    assert_eq!(eval("5; for (let k in {}) {}"), ValueView::Undefined);
    assert_eq!(eval("5; for (let k in null) 6;"), ValueView::Undefined);
    assert_eq!(eval("5; for (let k in {a: 1}) {}"), ValueView::Undefined);
    assert_eq!(num("5; for (let k in {a: 1, b: 2}) 6;"), 6.0);
    assert_eq!(num("5; for (let k in {a: 1, b: 2}) { 6; break; }"), 6.0);
    assert_eq!(
        eval("5; for (let k in {a: 1, b: 2}) { if (k === 'b') continue; 6; }"),
        ValueView::Undefined
    );
    assert_eq!(num("let i = 0; do { i = i + 1; } while (i < 3)"), 3.0);
    assert_eq!(num("let i = 0; do i = i + 1; while (i < 3) 4"), 4.0);
    assert_eq!(
        num("let i = 0; do { i = i + 1; if (i < 3) continue; 7; } while (i < 3)"),
        7.0
    );
    assert_eq!(
        eval("let i = 0; 1; do { i = i + 1; if (i == 2) { continue; } 8; } while (i < 2)"),
        ValueView::Undefined
    );
    assert_eq!(
        num("let i = 0; do { 6; try { i = i + 1; break; } finally { 7; } } while (true)"),
        1.0
    );
    // `for` (clj-quickjs-81): reset to undefined on entry, then the value
    // the body produced last; the head's init, test and update never set it.
    assert_eq!(num("5; for (;;) { 6; break; }"), 6.0);
    assert_eq!(eval("5; for (let i = 0; i < 0; ) {}"), ValueView::Undefined);
    assert_eq!(
        eval("let i; 5; for (i = 0; i < 2; i = i + 1) {}"),
        ValueView::Undefined
    );
    assert_eq!(num("for (let i = 0; i < 3; i = i + 1) { i * 2; }"), 4.0);
    // A `var` head (clj-quickjs-89) has no completion value either.
    assert_eq!(eval("5; for (var i = 0; i < 0; ) {}"), ValueView::Undefined);
    assert_eq!(num("for (var i = 7; i < 9; i++) i;"), 8.0);
    assert_eq!(
        num("for (let i = 0; i < 3; i = i + 1) { 9; continue; }"),
        9.0
    );
    // An `if` around the `continue` makes the value undefined.
    assert_eq!(
        eval("for (let i = 0; i < 3; i = i + 1) { if (i > 0) continue; 9; }"),
        ValueView::Undefined
    );
    assert_eq!(
        eval("1; for (let i = 0; i < 3; i = i + 1) { 2; if (i > 1) { break; } }"),
        ValueView::Undefined
    );
    // Labelled statements (clj-quickjs-82): the body's value; `break L`
    // keeps the value produced so far (LabelledEvaluation, UpdateEmpty).
    assert_eq!(num("5; L: { break L; }"), 5.0);
    assert_eq!(num("5; L: { 6; break L; }"), 6.0);
    assert_eq!(eval("5; L: if (true) break L;"), ValueView::Undefined);
    assert_eq!(num("5; L: 7;"), 7.0);
    // The inner loop's own value (undefined) replaces the 6 when `break L`
    // leaves it, as for an unlabelled `break` out of nested loops.
    assert_eq!(
        eval("5; L: while (true) { 6; while (true) { break L; } }"),
        ValueView::Undefined
    );
    assert_eq!(num("5; L: while (true) { 6; { break L; } }"), 6.0);
    assert_eq!(num("5; L: { try { 6; break L; } finally { 7; } }"), 6.0);
    // `switch` (clj-quickjs-83): undefined at entry, then the value of the
    // clauses that ran.
    assert_eq!(num("switch (1) { case 1: 7; case 2: 8; }"), 8.0);
    assert_eq!(eval("5; switch (1) { case 1: }"), ValueView::Undefined);
    assert_eq!(eval("5; switch (3) { case 1: 6; }"), ValueView::Undefined);
    assert_eq!(num("5; switch (1) { case 1: 6; break; case 2: 7; }"), 6.0);
    assert_eq!(
        num("5; switch (2) { case 1: 6; default: 7; case 2: 8; }"),
        8.0
    );
    assert_eq!(
        eval("5; switch (1) { case 1: 6; switch (2) { case 2: break; } }"),
        ValueView::Undefined
    );
    assert_eq!(num("5; L: switch (1) { case 1: 6; { break L; } }"), 6.0);
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

    // Jumps out of a `try` statement with a finally block run it on the way
    // (clj-quickjs-79; behaviour in `tests/exceptions.rs`
    // `break_and_continue_run_finally_blocks`).
    for (src, expected) in [
        ("let x = 1; while (x) { try { break; } finally {} } x", 1.0),
        (
            "let x = 1; while (x) { try { throw 0; } catch (e) { x = 0; continue; } finally {} } x",
            0.0,
        ),
        ("let x = 1; while (x) { try {} finally { break; } } x", 1.0),
        (
            "let x = 1; while (x) { try { try { x = 0; continue; } catch (e) {} } finally {} } x",
            0.0,
        ),
    ] {
        assert_eq!(num(src), expected, "{src}");
    }
}

/// `do`-`while` loops (clj-quickjs-80). `TEST_DO_WHILE` is adapted from
/// `test_do_while` in `research/quickjs/tests/test_loop.js` (QuickJS,
/// revision 7d1c13c5d8f8; see `docs/architecture.md` for the attribution):
/// `let` for `var`, `x = x + 1` for `x++`, and the asserted condition is
/// the completion value. The other scripts are written for this project.
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
fn do_while_loops() {
    const TEST_DO_WHILE: &str = "
        let i, c;
        i = 0;
        c = 0;
        do {
            c = c + 1;
            i = i + 1;
        } while (i < 3);
        c === 3 && i === 3";
    assert_eq!(eval(TEST_DO_WHILE), ValueView::Bool(true));
    // The same loop under a 3-instruction budget yields inside it and
    // resumes there with the same result.
    let looping = exe(TEST_DO_WHILE);
    let task = Runtime::new().spawn(&looping, &[]).unwrap();
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

    // The body runs once even when the test is false from the start.
    assert_eq!(num("let n = 0; do { n = n + 1; } while (false); n"), 1.0);
    // ASI after `while (...)` without a line break: two statements.
    assert_eq!(num("let n = 0; do n = n + 1; while (n < 4) n * 10"), 40.0);

    // Run under a bounded budget, so a `continue` that skipped the test
    // fails here instead of looping forever.
    let bounded = |src: &str| {
        let looping = exe(src);
        let task = Runtime::new().spawn(&looping, &[]).unwrap();
        match task
            .run_with(Budget::instructions(100_000), &mut NoEffects)
            .unwrap()
        {
            RunOutcome::Complete(v) => match v.view() {
                ValueView::Number(n) => n,
                other => panic!("{src}: expected a number, got {other:?}"),
            },
            other => panic!("{src}: expected Complete, got {other:?}"),
        }
    };
    // `continue` jumps to the test, which here holds the only update.
    assert_eq!(
        bounded(
            "let i = 0, n = 0; do { n = n + 1; continue; n = 99; } while ((i = i + 1) < 3); n * 10 + i"
        ),
        33.0
    );
    // `continue` skips the rest of the body: the odd numbers below 10.
    assert_eq!(
        bounded(
            "let i = 0, sum = 0;
             do {
                 i = i + 1;
                 if (i % 2 === 0) continue;
                 sum = sum + i;
             } while (i < 9);
             sum"
        ),
        25.0
    );
    // `break` leaves the loop before the test; only the innermost loop.
    assert_eq!(
        bounded(
            "let i = 0, log = 0;
             do {
                 i = i + 1;
                 let j = 0;
                 do { j = j + 1; if (j > i) break; log = log * 10 + j; } while (true);
                 if (i === 3) break;
                 log = log * 10;
             } while (true);
             log * 10 + i"
        ),
        // i = 1: 1; i = 2: 1 2; i = 3: 1 2 3, then the outer break.
        10120123.0 * 10.0 + 3.0
    );
    // Nested in a `while` loop, and a `while` loop nested in it.
    assert_eq!(
        bounded(
            "let a = 0, b = 0;
             while (a < 2) { a = a + 1; do { b = b + 1; continue; } while (false); }
             do { while (true) { break; } b = b * 10; } while (false);
             b"
        ),
        20.0
    );
    // Loop bodies make new bindings each iteration.
    assert_eq!(
        bounded(
            "let fs = 0, i = 0;
             do {
                 i = i + 1;
                 const k = i;
                 if (k === 1) { fs = () => k; continue; }
             } while (i < 3);
             fs() * 10 + i"
        ),
        13.0
    );
    // In a function: `return` from the body.
    assert_eq!(
        bounded(
            "function find(n) {
                 let i = 0;
                 do { if (i * i >= n) return i; i = i + 1; } while (true);
             }
             find(10) * 100 + find(0)"
        ),
        400.0
    );
    // Through `try` statements: a jump pops catch handlers and runs finally
    // blocks on the way (clj-quickjs-78, -79).
    assert_eq!(
        bounded(
            "let r = 0, n = 0;
             do { try { n = n + 1; continue; } finally { r = r + 1; } } while (n < 3);
             do { try { break; } catch (e) { r = 99; } } while (true);
             try { throw 4; } catch (e) { r = r * 10 + e; }
             r"
        ),
        34.0
    );
    assert_eq!(
        bounded(
            "let r = 0;
             do { try { throw 1; } catch (e) { r = e; break; } finally { r = r * 10; } } while (true);
             r"
        ),
        10.0
    );
    // The test runs outside the body's scope: it sees the outer binding.
    assert_eq!(
        bounded("let x = 2; do { let x = 0; } while ((x = x - 1) > 0); x"),
        0.0
    );

    // Completion values (ECMAScript UpdateEmpty).
    assert_eq!(num("5; do { 6; } while (false)"), 6.0);
    assert_eq!(eval("5; do { } while (false)"), ValueView::Undefined);
    assert_eq!(num("5; do { 6; break; } while (true)"), 6.0);
    assert_eq!(eval("5; do { break; } while (true)"), ValueView::Undefined);
}

/// `for` statements with empty, expression and `let`/`const` heads
/// (clj-quickjs-81). `TEST_FOR` is adapted from the first group of
/// `test_for` in `research/quickjs/tests/test_loop.js` (QuickJS, revision
/// 7d1c13c5d8f8; see `docs/architecture.md` for the attribution): `let` for
/// `var`, `x = x + 1` for `x++` in the body, and the asserted condition is
/// the completion value. The other scripts are written for this project.
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
fn for_loops() {
    const TEST_FOR: &str = "
        let i, c;
        c = 0;
        for(i = 0; i < 3; i++) {
            c = c + 1;
        }
        c === 3 && i === 3";
    assert_eq!(eval(TEST_FOR), ValueView::Bool(true));
    // The same loop under a 3-instruction budget yields inside it and
    // resumes there with the same result.
    let looping = exe(TEST_FOR);
    let task = Runtime::new().spawn(&looping, &[]).unwrap();
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

    // Run under a bounded budget, so a loop that fails to stop fails here
    // instead of running forever.
    let bounded = |src: &str| {
        let looping = exe(src);
        let task = Runtime::new().spawn(&looping, &[]).unwrap();
        match task
            .run_with(Budget::instructions(100_000), &mut NoEffects)
            .unwrap()
        {
            RunOutcome::Complete(v) => v.view(),
            other => panic!("{src}: expected Complete, got {other:?}"),
        }
    };
    let bounded_num = |src: &str| match bounded(src) {
        ValueView::Number(n) => n,
        other => panic!("{src}: expected a number, got {other:?}"),
    };
    // An empty head loops until `break`.
    assert_eq!(bounded("for (;;) { break; } 1"), ValueView::Number(1.0));
    assert_eq!(
        bounded_num("let n = 0; for (;;) { n = n + 1; if (n === 4) break; } n"),
        4.0
    );
    // `continue` runs the update: the sum of the odd numbers below 10.
    assert_eq!(
        bounded_num(
            "let sum = 0;
             for (let i = 0; i < 10; i = i + 1) {
                 if (i % 2 === 0) continue;
                 sum = sum + i;
             }
             sum"
        ),
        25.0
    );
    // The test is absent (always true) or false from the start; the update
    // is absent.
    assert_eq!(
        bounded_num("let n = 0; for (let i = 0; ; i = i + 1) { if (i > 2) break; n = n + 1; } n"),
        3.0
    );
    assert_eq!(
        bounded_num("let n = 0; for (let i = 5; i < 3; ) { n = 1; } n"),
        0.0
    );
    assert_eq!(
        bounded_num("let i = 0; for (; i < 4; ) { i = i + 1; } i"),
        4.0
    );
    // Several declarators, and an expression head with the comma operator.
    assert_eq!(
        bounded_num("let r = 0; for (let i = 0, j = 10; i < j; i = i + 1, j = j - 1) r = r + 1; r"),
        5.0
    );
    assert_eq!(
        bounded_num("let a, b; for (a = 1, b = 2; a < 3; a = a + 1) b = b * 10; b"),
        200.0
    );
    // The head's bindings are scoped to the loop: the outer one is untouched,
    // and a body block may shadow them.
    assert_eq!(
        bounded_num("let i = 7; for (let i = 0; i < 3; i = i + 1) { let x = i; } i"),
        7.0
    );
    assert_eq!(
        bounded_num("let s = 0; for (let i = 0; i < 2; i = i + 1) { let i = 5; s = s + i; } s"),
        10.0
    );
    // Nested loops: `break` and `continue` act on the innermost one.
    assert_eq!(
        bounded_num(
            "let log = 0;
             for (let i = 1; i <= 3; i = i + 1) {
                 for (let j = 1; ; j = j + 1) {
                     if (j > i) break;
                     if (j === 2) continue;
                     log = log * 10 + j;
                 }
             }
             log"
        ),
        // i = 1: 1; i = 2: 1; i = 3: 1 3.
        1113.0
    );
    // Through `try` statements: a jump pops catch handlers and runs finally
    // blocks on the way; the update still runs after `continue`.
    assert_eq!(
        bounded_num(
            "let r = 0;
             for (let i = 0; i < 3; i = i + 1) { try { continue; } finally { r = r + 1; } }
             for (;;) { try { break; } catch (e) { r = 99; } }
             r"
        ),
        3.0
    );
    // In a function: `return` from the body.
    assert_eq!(
        bounded_num(
            "function find(n) { for (let i = 0; ; i = i + 1) { if (i * i >= n) return i; } }
             find(10) * 100 + find(0)"
        ),
        400.0
    );

    // Per-iteration bindings: each closure keeps its iteration's `i`.
    assert_eq!(
        bounded_num(
            "const b = __rt.vectorBuilder();
             for (let i = 0; i < 3; i = i + 1) { __rt.builderConj(b, function () { return i; }); }
             const v = __rt.freeze(b);
             __rt.nth(v, 0)() * 100 + __rt.nth(v, 1)() * 10 + __rt.nth(v, 2)()"
        ),
        12.0
    );
    // An assignment made by a closure during an iteration is carried into
    // the next iteration's copy.
    assert_eq!(
        bounded_num(
            "let n = 0;
             for (let i = 0; i < 5; i = i + 1) { const f = function () { i = i + 1; }; f(); n = n + 1; }
             n"
        ),
        3.0
    );
    // A closure sees later assignments within its own iteration (and the
    // update's, not at all): closures created in the test and the update
    // capture the iteration they run in.
    assert_eq!(
        bounded_num(
            "const b = __rt.vectorBuilder();
             for (let i = 0; i < 3; i = i + 1) {
                 __rt.builderConj(b, () => i);
                 i = i + 1;
             }
             const v = __rt.freeze(b);
             __rt.count(v) * 100 + __rt.nth(v, 0)() * 10 + __rt.nth(v, 1)()"
        ),
        // Iterations with i = 0 (ends at 1) and i = 2 (ends at 3).
        213.0
    );
    assert_eq!(
        bounded_num(
            "let g = 0, h = 0;
             for (let i = 0; (g = () => i, i < 2); h = () => i, i = i + 1) {}
             g() * 10 + h()"
        ),
        // The last test ran in the iteration with i = 2; the last update
        // ran in that iteration too, before incrementing.
        22.0
    );
    // A closure in an initializer captures the head's first environment,
    // which the first iteration copies (CreatePerIterationEnvironment
    // before the first test), so it never sees the body's assignments.
    assert_eq!(
        bounded_num(
            "let f; for (let i = 0, g = () => i; i < 3; i = i + 1) { f = g; i = i + 10; } f()"
        ),
        0.0
    );
    // A `const` head is a fresh binding per loop, never updated.
    assert_eq!(
        bounded_num("let n = 0; for (const k = 3; n < k; ) { const f = () => k; n = n + f(); } n"),
        3.0
    );

    // Errors: the head's bindings are in TDZ during their own initializers;
    // a `const` head cannot be updated.
    let (kind, message, line, column) = failure_at("for (let i = i; ;) {}");
    assert_eq!(kind, VmErrorKind::ReferenceError);
    assert_eq!(message, "cannot access 'i' before initialization");
    assert_eq!((line, column), (1, 14));
    let (kind, ..) = failure_at("for (let i = 0, j = k, k = 1; ;) {}");
    assert_eq!(kind, VmErrorKind::ReferenceError);
    // A closure that runs during the initializer fails at run time.
    let (kind, ..) = failure_at("for (let i = (() => i)(); ;) {}");
    assert_eq!(kind, VmErrorKind::ReferenceError);
    let (kind, message, line, column) = failure_at("for (const k = 0; k < 1; k = k + 1) {}");
    assert_eq!(kind, VmErrorKind::TypeError);
    assert_eq!(message, "assignment to constant variable 'k'");
    assert_eq!((line, column), (1, 26));
    // The head bindings are not visible after the loop.
    let (kind, ..) = failure_at("for (let i = 0; i < 1; i = i + 1) {} i");
    assert_eq!(kind, VmErrorKind::ReferenceError);

    // Completion values (ECMAScript UpdateEmpty).
    assert_eq!(num("5; for (;;) { 6; break; }"), 6.0);
    assert_eq!(eval("5; for (let i = 0; i < 0; ) {}"), ValueView::Undefined);
    assert_eq!(eval("5; for (;;) { break; }"), ValueView::Undefined);
    assert_eq!(num("for (let i = 0; i < 3; i = i + 1) { i; }"), 2.0);
    // The head's expressions are never the completion value.
    assert_eq!(
        eval("let i; 5; for (i = 0; i < 2; i = i + 1) {}"),
        ValueView::Undefined
    );

    // Deeply nested loops compile within the 2 MiB stack the parser is
    // tested with.
    let nested = format!("{}x;", "for (let i = 0; i < 1; i = i + 1) ".repeat(120));
    let handle = std::thread::Builder::new()
        .stack_size(2 << 20)
        .spawn(move || compile_script("nested", &nested).map(|_| ()))
        .unwrap();
    handle.join().unwrap().unwrap();
}

/// `for`-`in` over own enumerable properties (clj-quickjs-85). `TEST_FOR_IN`
/// is adapted from the first group of `test_for_in`, and `TEST_FOR_IN2`
/// from `test_for_in2`, in `research/quickjs/tests/test_loop.js` (QuickJS,
/// revision 7d1c13c5d8f8; see `docs/architecture.md` for the attribution):
/// `let` for `var`, the keys collected with string `+` instead of
/// `tab.push` and `tab.toString()`, and the asserted condition is the
/// completion value. The other scripts are written for this project.
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
fn for_in_over_own_properties() {
    const TEST_FOR_IN: &str = r#"
        let i, tab;
        tab = "";
        for(i in {x:1, y: 2}) {
            tab = tab + i + ",";
        }
        tab === "x,y,""#;
    assert_eq!(eval(TEST_FOR_IN), ValueView::Bool(true));
    const TEST_FOR_IN2: &str = r#"
        let i, tab, ok;
        tab = "";
        for(i in {x:1, y: 2, z:3}) {
            if (i === "y")
                continue;
            tab = tab + i;
        }
        ok = tab === "xz";

        tab = "";
        for(i in {x:1, y: 2, z:3}) {
            if (i === "z")
                break;
            tab = tab + i;
        }
        ok && tab === "xy""#;
    assert_eq!(eval(TEST_FOR_IN2), ValueView::Bool(true));

    // Integer-index keys ascending, then the other keys in creation order;
    // keys are strings. `let` and `const` heads; an identifier head keeps
    // the last key.
    assert_eq!(
        string(r#"let s = ""; for (let k in {b:1, "2":1, a:1, "1":1}) s = s + k + ";"; s"#),
        "1;2;b;a;"
    );
    assert_eq!(
        string(r#"let s = ""; for (const k in {b:1, 10:1, 9:1}) s = s + typeof k + k; s"#),
        "string9string10stringb"
    );
    assert_eq!(string("let k; for (k in {p: 1, q: 2}) {} k"), "q");
    assert_eq!(string("let k = 'z'; for ((k) in {}) {} k"), "z");
    // `null`, `undefined`, numbers and booleans run zero iterations; a
    // string visits its indices.
    for src in ["null", "undefined", "0", "42", "true", "false", "''"] {
        assert_eq!(
            num(&format!("let n = 0; for (let k in {src}) n = n + 1; n")),
            0.0,
            "{src}"
        );
    }
    assert_eq!(
        string(r#"let s = ""; for (let k in "ab") s = s + k + typeof k; s"#),
        "0string1string"
    );
    // Non-enumerable properties are skipped; a key made non-enumerable
    // before it is reached is skipped; a key added during the loop is not
    // visited.
    assert_eq!(
        string(
            r#"let o = {a: 1};
               Object.defineProperty(o, "h", {value: 1, enumerable: false});
               o.b = 2; o.c = 3;
               let s = "";
               for (let k in o) {
                   if (k === "a") Object.defineProperty(o, "c", {enumerable: false});
                   o.d = 4;
                   s = s + k;
               }
               s"#
        ),
        "ab"
    );
    // A property deleted (clj-quickjs-124) before it is reached is not
    // visited; one deleted after its visit changes nothing.
    assert_eq!(
        string(
            r#"let o = {a: 1, b: 2, c: 3, d: 4};
               let s = "";
               for (let k in o) { if (k === "a") delete o.c; if (k === "b") delete o.a; s = s + k; }
               s + Object.keys(o).length"#
        ),
        "abd2"
    );
    // A function's own enumerable properties; an object's inherited
    // enumerable properties are visited too (clj-quickjs-86,
    // `for_in_walks_the_prototype_chain`).
    assert_eq!(
        string(r#"function f() {} f.x = 1; let s = ""; for (let k in f) s = s + k; s"#),
        "x"
    );
    assert_eq!(
        string(r#"let s = ""; for (let k in Object.create({p: 1})) s = s + k; s + "-""#),
        "p-"
    );

    // Each iteration has its own `let`/`const` binding: closures created
    // per iteration return their own key, also when the body assigns it.
    assert_eq!(
        string(
            r#"const b = __rt.vectorBuilder();
               for (let k in {x: 1, y: 2, z: 3}) { __rt.builderConj(b, () => k); }
               const v = __rt.freeze(b);
               __rt.nth(v, 0)() + __rt.nth(v, 1)() + __rt.nth(v, 2)()"#
        ),
        "xyz"
    );
    assert_eq!(
        string(
            r#"const b = __rt.vectorBuilder();
               for (let k in {x: 1, y: 2}) { __rt.builderConj(b, () => k); k = k + "!"; }
               const v = __rt.freeze(b);
               __rt.nth(v, 0)() + __rt.nth(v, 1)()"#
        ),
        "x!y!"
    );
    assert_eq!(
        string(
            r#"const b = __rt.vectorBuilder();
               for (const k in {x: 1, y: 2}) __rt.builderConj(b, function () { return k; });
               const v = __rt.freeze(b);
               __rt.nth(v, 0)() + __rt.nth(v, 1)()"#
        ),
        "xy"
    );
    // The head's binding is scoped to the loop.
    assert_eq!(string("let k = 'out'; for (let k in {a: 1}) {} k"), "out");

    // The object expression sees the head's binding in its temporal dead
    // zone, statically and through a closure created there.
    let (kind, _, line, column) = failure_at("for (let k in k) {}");
    assert_eq!((kind, line, column), (VmErrorKind::ReferenceError, 1, 15));
    let (kind, ..) =
        failure_at("let f; for (let k in (f = () => k, {a: 1})) { if (k !== 'a') throw 1; } f()");
    assert_eq!(kind, VmErrorKind::ReferenceError);
    // Assigning a `const` head is a TypeError; an undeclared target a
    // ReferenceError (strict code).
    let (kind, ..) = failure_at("for (const k in {a: 1}) k = 1;");
    assert_eq!(kind, VmErrorKind::TypeError);
    let (kind, ..) = failure_at("for (nope in {a: 1}) {}");
    assert_eq!(kind, VmErrorKind::ReferenceError);
    // An undeclared target with nothing to enumerate is never assigned.
    assert_eq!(eval("for (nope in {}) {} 1"), ValueView::Number(1.0));

    // Labels, nested loops, `try`/`finally`, `return` and exceptions.
    assert_eq!(
        string(
            r#"let s = "";
               outer: for (let i in {a: 1, b: 1, c: 1}) {
                   for (let j in {x: 1, y: 1}) {
                       if (i === "b") continue outer;
                       if (i === "c" && j === "y") break outer;
                       s = s + i + j;
                   }
               }
               s"#
        ),
        "axaycx"
    );
    assert_eq!(
        string(
            r#"let s = "";
               for (let k in {a: 1, b: 1, c: 1}) {
                   try { if (k === "b") continue; if (k === "c") break; s = s + k; }
                   finally { s = s + "f"; }
               }
               s"#
        ),
        "afff"
    );
    assert_eq!(
        string(
            r#"function first(o) { for (let k in o) { try { return k; } finally { o.seen = 1; } } }
               const o = {q: 1, r: 2};
               first(o) + o.seen"#
        ),
        "q1"
    );
    assert_eq!(
        string(
            r#"let s = "";
               try { for (let k in {a: 1, b: 1}) { s = s + k; throw s; } } catch (e) { s = s + "!" + e; }
               for (let k in {c: 1}) s = s + k;
               s"#
        ),
        "a!ac"
    );
    // A for-in loop inside a loop body runs afresh each time.
    assert_eq!(
        num(
            "let n = 0; for (let i = 0; i < 3; i = i + 1) for (let k in {a: 1, b: 2}) n = n + 1; n"
        ),
        6.0
    );
}

/// `for`-`in` walks the prototype chain (clj-quickjs-86). `TEST_FOR_IN_PROTO`
/// is adapted from the second and third groups of `test_for_in`, in
/// `research/quickjs/tests/test_loop.js` (QuickJS, revision 7d1c13c5d8f8;
/// see `docs/architecture.md` for the attribution). Modified: `let` for
/// `var`, the keys collected with string `+` instead of `tab.push` and
/// `tab.toString()`, and the asserted condition is the script's completion
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
fn for_in_walks_the_prototype_chain() {
    const TEST_FOR_IN_PROTO: &str = r#"
        let i, tab, a, b, ok;

        /* prototype chain test */
        a = {x:2, y: 2, "1": 3};
        b = {"4" : 3 };
        Object.setPrototypeOf(a, b);
        tab = "";
        for(i in a) {
            tab = tab + i + ",";
        }
        ok = tab === "1,x,y,4,";

        /* non enumerable properties hide enumerables ones in the
           prototype chain */
        a = {y: 2, "1": 3};
        Object.defineProperty(a, "x", { value: 1 });
        b = {"x" : 3 };
        Object.setPrototypeOf(a, b);
        tab = "";
        for(i in a) {
            tab = tab + i + ",";
        }
        ok && tab === "1,y,""#;
    assert_eq!(eval(TEST_FOR_IN_PROTO), ValueView::Bool(true));

    let keys = |src: &str| string(&format!(r#"let s = ""; {src} s"#));
    // A three-level chain: each object's own keys in OrdinaryOwnPropertyKeys
    // order, then its prototype's; a name is visited once, at the nearest
    // level (here with the nearest value), even where a farther level has
    // it as an index or makes it non-enumerable.
    assert_eq!(
        keys(
            r#"const c = {d: "c", 2: "c", z: "c"};
               Object.defineProperty(c, "w", {value: "c", enumerable: true});
               const b = Object.create(c); b.d = "b"; b.y = "b"; b[2] = "b";
               Object.defineProperty(b, "z", {value: "b", enumerable: false});
               const a = Object.create(b); a.x = "a"; a.d = "a";
               for (const k in a) s = s + k + "=" + a[k] + ";";"#
        ),
        "x=a;d=a;2=b;y=b;w=c;"
    );
    // A non-enumerable property of the middle level hides the top level's
    // enumerable one; a non-enumerable own property is not visited either.
    assert_eq!(
        keys(
            r#"const top = {h: 1, t: 1};
               const mid = Object.create(top);
               Object.defineProperty(mid, "h", {value: 1});
               const o = Object.create(mid);
               Object.defineProperty(o, "own", {value: 1});
               for (const k in o) s = s + k;"#
        ),
        "t"
    );
    // A prototype key deleted (or made non-enumerable) during the loop
    // before it is reached is not visited; one deleted after its visit
    // changes nothing; a key added to a prototype the walk has not reached
    // yet is visited when it is reached, one added to the object the walk
    // is on is not.
    assert_eq!(
        keys(
            r#"const p = {q: 1, r: 1, u: 1, v: 1};
               const o = {a: 1, __proto__: p};
               for (const k in o) {
                   if (k === "a") { delete p.r; p.late = 1; }
                   if (k === "q") { delete p.q; Object.defineProperty(p, "u", {enumerable: false}); p.later = 1; }
                   s = s + k + ",";
               }"#
        ),
        "a,q,v,late,"
    );
    // Deleting an own key does not unhide it: the prototype's key of the
    // same name stays hidden (the visited names are those of the snapshot).
    assert_eq!(
        keys(
            r#"const o = {a: 1, b: 1, __proto__: {b: 2, c: 2}};
               for (const k in o) { if (k === "a") delete o.b; s = s + k; }"#
        ),
        "ac"
    );
    // An object with a `null` prototype enumerates only its own keys; so
    // does one inheriting only from `Object.prototype` (whose properties are
    // all non-enumerable).
    assert_eq!(
        keys(
            r#"const o = Object.create(null); o.k = 1; o[0] = 1;
               for (const k in o) s = s + k; for (const k in {m: 1}) s = s + k;"#
        ),
        "0km"
    );
    assert_eq!(
        keys(r#"for (const k in Object.prototype) s = s + k; for (const k in Object) s = s + k;"#),
        ""
    );
    // The chain can change during the loop: the walk continues on the
    // prototype it is on, and then follows that prototype's own chain.
    assert_eq!(
        keys(
            r#"const top = {t: 1};
               const p = {p: 1, p2: 1, __proto__: top};
               const o = {o: 1, __proto__: p};
               for (const k in o) { if (k === "p") Object.setPrototypeOf(o, null); s = s + k; }"#
        ),
        "opp2t"
    );
    // The chain is read lazily: a prototype replaced (or removed) while the
    // walk is still on the object's own keys is not visited; the new one is.
    assert_eq!(
        keys(
            r#"const old = {gone: 1};
               const o = {a: 1, b: 1, __proto__: old};
               for (const k in o) { if (k === "a") Object.setPrototypeOf(o, {fresh: 1, b: 2}); s = s + k + ","; }
               const n = {a: 1, __proto__: {gone: 1}};
               for (const k in n) { Object.setPrototypeOf(n, null); s = s + k + ","; }"#
        ),
        "a,b,fresh,a,"
    );
    // A function on the chain contributes its own enumerable properties;
    // the walk ends there (`Function.prototype` has no enumerable ones).
    assert_eq!(
        keys(
            r#"function f() {} f.fx = 1;
               const o = Object.create(f); o.own = 1;
               for (const k in o) s = s + k;"#
        ),
        "ownfx"
    );
    // Arrays on the chain: elements and named properties; `length` (own,
    // non-enumerable) hides an inherited enumerable `length`.
    assert_eq!(
        keys(
            r#"const arr = [7, 8]; arr.n = 1;
               const o = Object.create(arr); o[5] = 1;
               for (const k in o) s = s + k + ",";
               const q = Object.create({length: 1, z: 1}); Object.setPrototypeOf(arr, q);
               for (const k in arr) s = s + k + ",";"#
        ),
        "5,0,1,n,0,1,n,z,"
    );
}

/// `for`-`in` over arrays (clj-quickjs-88). `TEST_FOR_IN_ARRAY` is adapted
/// from the fourth group ("array optimization") of `test_for_in`, in
/// `research/quickjs/tests/test_loop.js` (QuickJS, revision 7d1c13c5d8f8;
/// see `docs/architecture.md` for the attribution). Modified: `let` for
/// `var`, `a[a.length] = i` for `a.push(i)`, the keys collected with string
/// `+` instead of `tab.push` and `tab.toString()` (`Array.prototype.push`,
/// `join` and `toString` are clj-quickjs-155 and -156), and the asserted
/// condition is the script's completion value, with a string-key check
/// added. The other scripts are written for this project.
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
fn for_in_over_arrays() {
    const TEST_FOR_IN_ARRAY: &str = r#"
        let i, tab, a;
        let strings = true;

        /* array optimization */
        a = [];
        for(i = 0; i < 10; i++)
            a[a.length] = i;
        tab = "";
        for(i in a) {
            strings = strings && typeof i === "string";
            tab = tab + i + ",";
        }
        strings && tab === "0,1,2,3,4,5,6,7,8,9,""#;
    assert_eq!(eval(TEST_FOR_IN_ARRAY), ValueView::Bool(true));

    let keys = |src: &str| string(&format!(r#"let s = ""; {src} s"#));
    // Each key is a string (not the index number), and `length` (own,
    // non-enumerable) is not visited.
    assert_eq!(
        keys(r#"for (const k in [5, 6]) s = s + typeof k + ":" + (k === "0" || k === "1") + ",";"#),
        "string:true,string:true,"
    );
    // Holes are skipped, wherever they are: elisions, a deleted element, and
    // the gaps index assignments leave (sparse storage, clj-quickjs-118).
    assert_eq!(
        keys(r#"for (const k in [1, , 3]) s = s + k + ",";"#),
        "0,2,"
    );
    assert_eq!(
        keys(
            r#"const a = [, 1, , , 4, 5, ]; delete a[5];
               for (const k in a) s = s + k + ",";
               const b = []; b[3] = 1; b[0] = 1; b[1000] = 1; b[4294967294] = 1;
               for (const k in b) s = s + k + ",";"#
        ),
        "1,4,0,3,1000,4294967294,"
    );
    // Named properties come after the indices, in creation order, whatever
    // order they were added in; an index added after a name is still first.
    assert_eq!(
        keys(
            r#"const a = [1, 2]; a.extra = 1; a.b = 1; a[2] = 1;
               for (const k in a) s = s + k + ",";"#
        ),
        "0,1,2,extra,b,"
    );
    // Index-like names that are not array indices are named properties:
    // `01`, 2**32 - 1, `-1` and `1.5` follow the indices in creation order.
    assert_eq!(
        keys(
            r#"const a = [0]; a["01"] = 1; a[4294967295] = 1; a[-1] = 1; a[1.5] = 1; a[4294967294] = 1;
               for (const k in a) s = s + k + ",";"#
        ),
        "0,4294967294,01,4294967295,-1,1.5,"
    );
    // Shrinking the array removes elements before they are reached: the
    // enumerator re-checks each key as an own enumerable property. Growing
    // the array adds no keys (the indices are snapshotted when the loop
    // starts); an element deleted and put back before it is reached is
    // visited. Array.prototype.pop is not implemented; its shrink row
    // remains in clj-quickjs-264.
    assert_eq!(
        keys(
            r#"const a = [0, 1, 2, 3, 4];
               for (const k in a) { if (k === "0") a.length = 2; s = s + k + ","; }
               const c = [0, 1];
               for (const k in c) { c[c.length] = 9; s = s + k + ","; }
               const d = [0, 1, 2, 3];
               for (const k in d) { if (k === "0") { delete d[1]; delete d[2]; d[2] = 2; } s = s + k + ","; }"#
        ),
        "0,1,0,1,0,2,3,"
    );
    // Shrinking sparse storage to zero also skips removed indices, but
    // still visits named properties after the current index.
    assert_eq!(
        keys(
            r#"const a = []; a[3] = 3; a[4000] = 4; a.extra = 1;
               for (const k in a) { if (k === "3") a.length = 0; s = s + k + ","; }
               s = s + a.length;"#
        ),
        "3,extra,0"
    );
    // A plain identifier head and a `var` head see the same keys; the
    // element values are read through the keys.
    assert_eq!(
        keys(
            r#"let k; const a = ["x", "y"]; a.z = "w";
               for (k in a) s = s + k + "=" + a[k] + ";";
               for (var v in a) s = s + v;"#
        ),
        "0=x;1=y;z=w;01z"
    );
}

/// `var` heads of `for` and `for`-`in` statements (clj-quickjs-89).
/// `TEST_FOR_VAR` is the second group of `test_for`, and `TEST_FOR_IN_VAR`
/// is adapted from the seventh group of `test_for_in`, in
/// `research/quickjs/tests/test_loop.js` (QuickJS, revision 7d1c13c5d8f8;
/// see `docs/architecture.md` for the attribution). Modified: the keys are
/// collected with string `+` instead of `tab.push` and `tab.toString()`,
/// and the asserted condition is the script's completion value. The other
/// scripts are written for this project.
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
fn var_heads_in_for_loops() {
    const TEST_FOR_VAR: &str = "
        var c;
        c = 0;
        for(var j = 0; j < 3; j++) {
            c++;
        }
        c === 3 && j === 3";
    assert_eq!(eval(TEST_FOR_VAR), ValueView::Bool(true));
    // The same loop inside a function, under a 3-instruction budget: it
    // yields inside the loop and resumes there with the same result.
    let looping = exe(
        "function f() { var c; c = 0; for (var j = 0; j < 3; j++) { c++; } return c === 3 && j === 3; } f()",
    );
    let task = Runtime::new().spawn(&looping, &[]).unwrap();
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

    // Hoisting: the binding exists and is `undefined` before the loop, in a
    // script and in a function, also when the loop never runs its head.
    assert_eq!(
        eval("const before = j; for (var j = 0; j < 3; j = j + 1) {} before"),
        ValueView::Undefined
    );
    assert_eq!(
        num(
            "function f() { const before = j; for (var j = 5; j < 7; j = j + 1) {} \
             return before === undefined ? j : -1; } f()"
        ),
        7.0
    );
    assert_eq!(
        eval("function f(go) { if (go) for (var k = 1; ; ) break; return k; } f(false)"),
        ValueView::Undefined
    );
    // Several declarators; one without an initializer keeps its value.
    assert_eq!(
        num("var b = 4; for (var a = 0, b; a < 2; a = a + 1) b = b * 10; a * 1000 + b"),
        2400.0
    );
    // The head's `var` is the function's binding: assignments in the body,
    // in nested blocks and through a closure all reach it, and a later loop
    // with the same `var` head reuses it.
    assert_eq!(
        num(
            "function f() { for (var i = 0; i < 10; i = i + 1) { { if (i === 3) break; } } \
             for (var i = i * 2; i < 100; i = i * 10) {} return i; } f()"
        ),
        600.0
    );
    assert_eq!(
        num("function f(i) { for (var i = i + 1; i < 5; i = i + 1) {} return i; } f(2)"),
        5.0
    );
    // No per-iteration copy: closures created in the body all share the one
    // binding and see its final value (with a `let` head each keeps its own
    // iteration's value: `for_loops`).
    assert_eq!(
        num("const b = __rt.vectorBuilder();
             for (var i = 0; i < 3; i = i + 1) { __rt.builderConj(b, function () { return i; }); }
             const v = __rt.freeze(b);
             __rt.nth(v, 0)() * 100 + __rt.nth(v, 1)() * 10 + __rt.nth(v, 2)()"),
        333.0
    );
    // A closure that assigns the binding steers the loop.
    assert_eq!(
        num(
            "let n = 0;
             for (var i = 0; i < 10; i = i + 1) { const skip = function () { i = i + 2; }; skip(); n = n + 1; }
             n * 100 + i"
        ),
        412.0
    );
    // `break` and `continue` leave the binding with its current value.
    assert_eq!(
        num("for (var i = 0; ; i = i + 1) { if (i < 4) continue; break; } i"),
        4.0
    );

    // For-in `var` heads: each key is assigned to the one function-scoped
    // binding, which keeps the last key after the loop.
    const TEST_FOR_IN_VAR: &str = r#"
        var tab;
        tab = "";
        for(var j in {x:1, y: 2}) {
            tab = tab + j + ",";
        }
        tab === "x,y,""#;
    assert_eq!(eval(TEST_FOR_IN_VAR), ValueView::Bool(true));
    assert_eq!(
        eval(r#"for (var k in {x:1, y:2}) {} k === "y""#),
        ValueView::Bool(true)
    );
    assert_eq!(
        string(r#"function f() { for (var k in {x:1, y:2}) {} return k; } f()"#),
        "y"
    );
    // Hoisted: `undefined` before the loop, and after a loop over no keys.
    assert_eq!(
        eval(r#"const before = k; for (var k in {a: 1}) {} before === undefined && k === "a""#),
        ValueView::Bool(true)
    );
    assert_eq!(
        eval("function f() { for (var k in {}) {} return k; } f()"),
        ValueView::Undefined
    );
    // `break` leaves the binding at the current key; an assignment in the
    // body does not change the next key.
    assert_eq!(
        string(r#"for (var k in {a: 1, b: 2, c: 3}) { if (k === "b") break; } k"#),
        "b"
    );
    assert_eq!(
        string(r#"let s = ""; for (var k in {a: 1, b: 2}) { s = s + k; k = "z"; } s + k"#),
        "abz"
    );
    // No per-iteration copy: closures created in the body all see the last
    // key (with a `let` head each keeps its own: `for_in_over_own_properties`).
    assert_eq!(
        string(
            "const b = __rt.vectorBuilder();
             for (var k in {a: 1, b: 2, c: 3}) { __rt.builderConj(b, function () { return k; }); }
             const v = __rt.freeze(b);
             __rt.nth(v, 0)() + __rt.nth(v, 1)() + __rt.nth(v, 2)()"
        ),
        "ccc"
    );
    // A for-in `var` loop inside a function, under a 3-instruction budget:
    // it yields inside the loop and resumes there with the same result.
    let looping = exe(
        r#"function f() { var s = ""; for (var k in {a: 1, b: 2, c: 3}) { s = s + k; } return s + k; } f()"#,
    );
    let task = Runtime::new().spawn(&looping, &[]).unwrap();
    let mut yields = 0;
    let result = loop {
        match task
            .run_with(Budget::instructions(3), &mut NoEffects)
            .unwrap()
        {
            RunOutcome::Yielded => yields += 1,
            RunOutcome::Complete(v) => break v.string().expect("a string value").to_string(),
            other => panic!("unexpected {other:?}"),
        }
    };
    assert_eq!(result, "abcc");
    assert!(yields >= 1, "{yields} yields");
}

/// Labelled statements and labelled `break`/`continue` (clj-quickjs-82).
/// `TEST_FOR_BREAK` is adapted from `test_for_break` and `TEST_CYCLIC_LABELS`
/// is the loop of `test_cyclic_labels`, both in
/// `research/quickjs/tests/test_loop.js`; `TEST_LABELS` and the two
/// `test_labels2` loops are adapted from `test_labels` and `test_labels2` in
/// `research/quickjs/tests/test_language.js` (QuickJS, revision
/// 7d1c13c5d8f8; see `docs/architecture.md` for the attribution).
/// Modified: `let` for `var`, `x = x + 1` for `x++`, the `with` statement of
/// `test_labels` is omitted, and the asserted condition is the script's
/// completion value. The other scripts are written for this project.
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
fn labelled_break_and_continue() {
    const TEST_FOR_BREAK: &str = "
        let i, c;
        c = 0;
        L1: for(i = 0; i < 3; i = i + 1) {
            c = c + 1;
            if (i == 0)
                continue;
            while (1) {
                break L1;
            }
        }
        c === 2 && i === 1";
    assert_eq!(eval(TEST_FOR_BREAK), ValueView::Bool(true));
    // The same loop under a 3-instruction budget yields inside it and
    // resumes there with the same result.
    let looping = exe(TEST_FOR_BREAK);
    let task = Runtime::new().spawn(&looping, &[]).unwrap();
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

    const TEST_LABELS: &str = "
        do x: { break x; } while(0);
        if (1)
            x: { break x; }
        else
            x: { break x; }
        while (0) x: { break x; };";
    assert_eq!(eval(TEST_LABELS), ValueView::Undefined);

    // `test_labels2`: an unlabelled `break` inside a labelled block leaves
    // the enclosing loop, not the block.
    assert_eq!(
        eval(
            "while (1) label: break
             let i = 0
             while (i < 3) label: {
                 if (i > 0)
                     break
                 i++
             }
             i === 1"
        ),
        ValueView::Bool(true)
    );
    assert_eq!(
        eval(
            "for (;;) label: break
             let i
             for (i = 0; i < 3; i++) label: {
                 if (i > 0)
                     break
             }
             i === 1"
        ),
        ValueView::Bool(true)
    );

    // `continue` to an outer labelled loop from nested loops, and through a
    // label set: each of `i` (0..3) adds the `j` values (0..i) before the
    // first `j === i`.
    assert_eq!(
        num("let s = 0;
             outer: for (let i = 0; i < 4; i++) {
                 let j = 0;
                 inner: while (true) {
                     if (j === i) continue outer;
                     do { s = s + j; j++; continue inner; } while (false);
                 }
             }
             s"),
        // i = 0: 0; i = 1: 0; i = 2: 0 + 1; i = 3: 0 + 1 + 2.
        4.0
    );
    assert_eq!(
        num("let n = 0, i = 0;
             a: b: while (i < 5) { i++; while (true) { if (i % 2) continue a; else continue b; } n = 99; }
             n * 10 + i"),
        5.0
    );
    // A labelled `do`-`while`: `continue L` evaluates its test, `break L`
    // skips it.
    let looping = exe("let i = 0, t = 0;
         L: do { i++; while (true) { if (i < 3) continue L; break L; } } while ((t = t + 1) < 5);
         i * 10 + t");
    let task = Runtime::new().spawn(&looping, &[]).unwrap();
    match task
        .run_with(Budget::instructions(100_000), &mut NoEffects)
        .unwrap()
    {
        RunOutcome::Complete(v) => assert_eq!(v.view(), ValueView::Number(32.0)),
        other => panic!("expected Complete, got {other:?}"),
    }
    // `break` to an outer label leaves every loop and block in between;
    // `break` to the label of a non-loop statement continues after it.
    assert_eq!(
        num("let n = 0;
             L: { while (true) { do { n++; if (n > 2) break L; } while (true); } n = 99; }
             n"),
        3.0
    );
    assert_eq!(
        num("let n = 0; L: if (true) { n = 1; break L; n = 2; } else n = 3; n"),
        1.0
    );
    assert_eq!(
        num("let n = 0; L: try { n = 1; break L; } catch (e) { n = 2; } n = n + 10; n"),
        11.0
    );
    // A labelled jump through finally blocks runs them, innermost first,
    // and a jump out of a catch body no longer has the handler.
    assert_eq!(
        num("let log = 0;
             L: { try { try { break L; } finally { log = log * 10 + 1; } } finally { log = log * 10 + 2; } log = 99; }
             log"),
        12.0
    );
    assert_eq!(
        num("let log = 0;
             outer: for (let i = 0; i < 3; i++) {
                 for (;;) {
                     try { if (i < 2) continue outer; break outer; } finally { log = log + 1; }
                 }
             }
             log"),
        3.0
    );
    assert_eq!(
        num("let r = 0;
             try {
                 L: try { throw 1; } catch (e) { break L; }
                 throw 2;
             } catch (e) { r = e; }
             r"),
        2.0
    );
    // A label in a function body; labels are not bindings.
    assert_eq!(
        num(
            "function f(x) { x: { if (x) break x; return 1; } return 2; }
             f(true) * 10 + f(false)"
        ),
        21.0
    );
    // Each closure created in a `for (let ...)` loop that `continue L` and
    // `break L` leave keeps its iteration's binding.
    assert_eq!(
        num("let fs = 0, g = 0;
             L: for (let i = 0; i < 3; i++) {
                 const h = () => i;
                 if (i === 0) { fs = h; continue L; }
                 g = h;
                 break L;
             }
             fs() * 10 + g()"),
        1.0
    );

    // Completion values (ECMAScript LabelledEvaluation and UpdateEmpty).
    assert_eq!(num("5; L: { break L; }"), 5.0);
    assert_eq!(num("5; L: { 6; break L; }"), 6.0);
    assert_eq!(eval("5; L: if (true) break L;"), ValueView::Undefined);

    // `test_cyclic_labels`'s loop never completes: it yields under every
    // budget until cancelled.
    const TEST_CYCLIC_LABELS: &str = "
        for (;;) {
            l: break l;
            l: break l;
            l: break l;
        }";
    let looping = exe(TEST_CYCLIC_LABELS);
    let task = Runtime::new().spawn(&looping, &[]).unwrap();
    for _ in 0..20 {
        match task
            .run_with(Budget::instructions(50), &mut NoEffects)
            .unwrap()
        {
            RunOutcome::Yielded => {}
            other => panic!("unexpected {other:?}"),
        }
    }
    assert!(matches!(task.cancel(), Cancel::Requested));
    match task
        .run_with(Budget::instructions(50), &mut NoEffects)
        .unwrap()
    {
        RunOutcome::Cancelled => {}
        other => panic!("unexpected {other:?}"),
    }

    // Label sets and labelled loops near the nesting limit compile within
    // the 2 MiB stack the parser is tested with.
    let labels: String = (0..250).map(|i| format!("l{i}: ")).collect();
    let loops: String = (0..60).map(|i| format!("l{i}: while (1) ")).collect();
    let handle = std::thread::Builder::new()
        .stack_size(2 << 20)
        .spawn(move || {
            let a = compile_script("labels", &format!("{labels}x;")).map(|_| ());
            let b = compile_script("loops", &format!("{loops}break l0;")).map(|_| ());
            (a, b)
        })
        .unwrap();
    let (a, b) = handle.join().unwrap();
    a.unwrap();
    b.unwrap();
}

/// `switch` statements (clj-quickjs-83). `TEST_SWITCH1` and `TEST_SWITCH2`
/// are adapted from `test_switch1` and `test_switch2` in
/// `research/quickjs/tests/test_loop.js` (QuickJS, revision 7d1c13c5d8f8;
/// see `docs/architecture.md` for the attribution): `let` for `var`, and
/// the asserted condition is the completion value. The numeric versions map
/// `"a"`, `"b"`, `"c"` to the digits 1, 2, 3 and append with
/// `s = s * 10 + d` (`a = i` for `a = "" + i`); the string versions keep
/// upstream's `s += a` (compound assignment, clj-quickjs-84).
/// The other scripts are written for this project.
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
fn switch_statements() {
    const TEST_SWITCH1: &str = "
        let i, a, s;
        s = 0;
        for(i = 0; i < 3; i++) {
            a = 0;
            switch(i) {
            case 0:
                a = 1;
                break;
            case 1:
                a = 2;
                break;
            default:
                a = 3;
                break;
            }
            s = s * 10 + a;
        }
        s === 123 && i === 3";
    assert_eq!(eval(TEST_SWITCH1), ValueView::Bool(true));
    const TEST_SWITCH2: &str = "
        let i, a, s;
        s = 0;
        for(i = 0; i < 4; i++) {
            a = 0;
            switch(i) {
            case 0:
                a = 1;
                break;
            case 1:
                a = 2;
                break;
            case 2:
                continue;
            default:
                a = i;
                break;
            }
            s = s * 10 + a;
        }
        s === 123 && i === 4";
    assert_eq!(eval(TEST_SWITCH2), ValueView::Bool(true));
    assert_eq!(
        eval(
            r#"
        let i, a, s;
        s = "";
        for(i = 0; i < 3; i++) {
            a = "?";
            switch(i) {
            case 0:
                a = "a";
                break;
            case 1:
                a = "b";
                break;
            default:
                a = "c";
                break;
            }
            s += a;
        }
        s === "abc" && i === 3"#
        ),
        ValueView::Bool(true)
    );
    assert_eq!(
        eval(
            r#"
        let i, a, s;
        s = "";
        for(i = 0; i < 4; i++) {
            a = "?";
            switch(i) {
            case 0:
                a = "a";
                break;
            case 1:
                a = "b";
                break;
            case 2:
                continue;
            default:
                a = "" + i;
                break;
            }
            s += a;
        }
        s === "ab3" && i === 4"#
        ),
        ValueView::Bool(true)
    );

    // The same loop under a 3-instruction budget yields inside the switch
    // and resumes there with the same result.
    let looping = exe(TEST_SWITCH2);
    let task = Runtime::new().spawn(&looping, &[]).unwrap();
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
    assert!(yields >= 10, "{yields} yields");

    // Matching is strict equality.
    assert_eq!(num("let r = 0; switch (0) { case -0: r = 1; } r"), 1.0);
    assert_eq!(
        num("let r = 0; switch (NaN) { case NaN: r = 1; break; default: r = 2; } r"),
        2.0
    );
    assert_eq!(
        num(r#"let r = 0; switch ("1") { case 1: r = 1; break; default: r = 2; } r"#),
        2.0
    );
    assert_eq!(
        num(r#"let r = 0; switch ("ab") { case "a": r = 1; break; case "a" + "b": r = 2; } r"#),
        2.0
    );
    assert_eq!(
        num("let r = 0; switch (null) { case undefined: r = 1; break; case null: r = 2; } r"),
        2.0
    );
    assert_eq!(
        num(
            "let r = 0; const f = () => 1; switch (f) { case () => 1: r = 1; break; case f: r = 2; } r"
        ),
        2.0
    );
    // No match and no `default`: nothing runs.
    assert_eq!(
        num("let r = 0; switch (3) { case 1: r = 1; case 2: r = 2; } r"),
        0.0
    );

    // The discriminant is evaluated once; the tests in source order until
    // one matches, and none after it.
    assert_eq!(
        num("let n = 0, d = 0;
             function t(v) { n = n * 10 + v; return v; }
             switch ((d = d + 1, 2)) { case t(1): case t(2): n = n * 10 + 9; break; case t(3): n = 0; }
             n * 10 + d"),
        1291.0
    );
    // With no match, every test runs before `default`, wherever it is.
    assert_eq!(
        num("let n = 0;
             function t(v) { n = n * 10 + v; return v; }
             switch (5) { case t(1): n = 0; default: n = n * 10 + 7; case t(2): n = n * 10 + 8; break; case t(3): n = 0; }
             n"),
        12378.0
    );

    // Fall-through, including from `default` placed before other clauses,
    // and `default` in the middle when a later case matches.
    assert_eq!(
        num(
            "let s = 0; switch (9) { default: s = s * 10 + 1; case 1: s = s * 10 + 2; break; case 2: s = 99; } s"
        ),
        12.0
    );
    assert_eq!(
        num(
            "let s = 0; switch (3) { case 1: s = 1; default: s = s * 10 + 2; case 3: s = s * 10 + 3; } s"
        ),
        3.0
    );
    assert_eq!(
        num(
            "let s = 0; switch (1) { case 1: s = s * 10 + 1; case 2: case 3: s = s * 10 + 3; default: s = s * 10 + 4; } s"
        ),
        134.0
    );

    // `break` leaves the innermost switch or loop; `continue` passes the
    // switch to its loop; a labelled `break` leaves a labelled switch.
    assert_eq!(
        num("let s = 0;
             switch (1) { case 1: switch (2) { case 2: s = 1; break; } s = s * 10 + 2; }
             s"),
        12.0
    );
    assert_eq!(
        num("let s = 0;
             switch (1) { case 1: for (let i = 0; ; i++) { if (i === 3) break; s++; } s = s * 10; }
             s"),
        30.0
    );
    assert_eq!(
        num("let s = 0;
             L: switch (1) { case 1: while (true) { s++; break L; } s = 99; }
             s"),
        1.0
    );
    assert_eq!(
        num("let s = 0, i = 0;
             L: while (i < 5) { i++; switch (i % 2) { case 0: continue L; default: s = s * 10 + i; } }
             s"),
        135.0
    );
    // Jumps out of a switch through finally blocks run them.
    assert_eq!(
        num("let log = 0;
             for (let i = 0; i < 3; i++) {
                 switch (i) {
                 case 1: try { continue; } finally { log = log * 10 + 1; }
                 default: try { break; } finally { log = log * 10 + 2; }
                 }
             }
             log"),
        212.0
    );
    // `return` from a clause.
    assert_eq!(
        num(
            "function f(x) { switch (x) { case 1: return 10; default: return 20; } }
             f(1) + f(2)"
        ),
        30.0
    );

    // The case block is one scope: function declarations are hoisted to
    // its start, so a later clause sees one declared in a skipped clause.
    assert_eq!(
        num("let r = 0; switch (1) { case 0: function f() { return 7; } case 1: r = f(); } r"),
        7.0
    );
    // Entering a clause past a `let`/`const` leaves it uninitialized: a
    // run-time ReferenceError at the reference or assignment.
    let cases = [
        ("switch (1) { case 0: let y = 1; case 1: y; }", "y", 1, 41),
        (
            "switch (1) { case 0: let y = 1; case 1: y = 2; }",
            "y",
            1,
            41,
        ),
        (
            "switch (1) {\ncase 0:\n  const c = 1;\ndefault:\ncase 1:\n  c;\n}",
            "c",
            6,
            3,
        ),
        // The tests run before every clause body.
        ("switch (1) { case y: let y = 1; }", "y", 1, 19),
        (
            "switch (1) { case 0: let y = 1; case 1: function g() { return y; } g(); }",
            "y",
            1,
            63,
        ),
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
    // Entered at its declaration's clause, the binding is initialized.
    assert_eq!(
        num("let r = 0; switch (0) { case 0: let y = 1; case 1: y = y + 1; r = y; } r"),
        2.0
    );
    // Each execution of the case block makes new bindings.
    assert_eq!(
        num("let s = 0;
             for (let i = 0; i < 3; i++) {
                 switch (i) { case 0: let y = 5; case 1: s = s * 10 + i; default: }
             }
             s"),
        1.0
    );
    assert_eq!(
        failure_at("for (let i = 0; i < 2; i++) { switch (i) { case 0: let y = 5; case 1: y; } }")
            .0,
        VmErrorKind::ReferenceError
    );
    // A closure created in one clause sees a `let` assigned in a later
    // clause, and fails if it runs before the declaration ran.
    assert_eq!(
        num("let f; switch (0) { case 0: f = () => x; case 1: let x = 1; case 2: x = 5; } f()"),
        5.0
    );
    assert_eq!(
        failure_at("switch (0) { case 0: let g = () => x; g(); case 1: let x = 1; }").0,
        VmErrorKind::ReferenceError
    );

    // Switches nested near the nesting limit compile within the 2 MiB stack
    // the parser is tested with.
    let nested = format!("{}x;{}", "switch (1) { case 1: ".repeat(60), "}".repeat(60));
    let handle = std::thread::Builder::new()
        .stack_size(2 << 20)
        .spawn(move || compile_script("switches", &nested).map(|_| ()))
        .unwrap();
    handle.join().unwrap().unwrap();
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

/// Directive prologues and sloppy compilation (clj-quickjs-95).
#[test]
fn use_strict_directives() {
    // A directive is still an expression statement with its string value.
    assert_eq!(num("\"use strict\"; 1"), 1.0);
    assert_eq!(string("\"use strict\";"), "use strict");
    assert_eq!(string("'use strict'"), "use strict");

    // Sloppy script, one strict function, an arrow nested in it.
    let src = "function sloppy(eval) { var arguments = 2; return 2; }
               function strict(a) { \"use strict\"; return () => a; }
               strict(1)() + sloppy(0)";
    let script =
        compile_script_with("sloppy", src, ScriptOptions::SLOPPY).unwrap_or_else(|e| panic!("{e}"));
    let sloppy = script.function(0).unwrap();
    let strict = script.function(1).unwrap();
    let arrow = strict.function(0).unwrap();
    assert_eq!(
        (sloppy.name(), strict.name(), arrow.name()),
        ("sloppy", "strict", "anonymous")
    );
    assert!(!script.is_strict());
    assert!(!sloppy.is_strict());
    assert!(strict.is_strict());
    assert!(arrow.is_strict());
    match run(&script, &[]) {
        RunOutcome::Complete(v) => assert_eq!(v.view(), ValueView::Number(3.0)),
        other => panic!("expected Complete, got {other:?}"),
    }
    // Strictness is part of the code: it survives the byte format.
    assert!(!Executable::load(sloppy.to_spec()).unwrap().is_strict());
    assert!(Executable::load(strict.to_spec()).unwrap().is_strict());

    // A script-level directive makes the script and its functions strict.
    let script = compile_script_with(
        "directive",
        "'use strict'; function f() { return 1; } f()",
        ScriptOptions::SLOPPY,
    )
    .unwrap();
    assert!(script.is_strict() && script.function(0).unwrap().is_strict());

    // Sloppy entry functions are strict only with their own directive.
    let entry =
        compile_function_with("function f(eval) { return 1; }", ScriptOptions::SLOPPY).unwrap();
    assert!(!entry.is_strict());
    match run(&entry, &[]) {
        RunOutcome::Complete(v) => assert_eq!(v.view(), ValueView::Number(1.0)),
        other => panic!("expected Complete, got {other:?}"),
    }
    let entry =
        compile_function_with("function f() { 'use strict'; }", ScriptOptions::SLOPPY).unwrap();
    assert!(entry.is_strict());

    // Sloppy bindings named `eval` and `arguments` are read and assigned
    // like any other binding.
    let sloppy = |src: &str| {
        let exe = compile_script_with("sloppy", src, ScriptOptions::SLOPPY)
            .unwrap_or_else(|e| panic!("{src:?}: {e}"));
        match run(&exe, &[]) {
            RunOutcome::Complete(v) => v.view(),
            other => panic!("{src:?}: expected Complete, got {other:?}"),
        }
    };
    for (src, expected) in [
        ("let eval = 1; eval = 2; eval", 2.0),
        (
            "function f(arguments) { arguments = 3; return arguments; } f(1)",
            3.0,
        ),
        (
            "function f(eval) { eval += 1; eval++; return eval; } f(1)",
            3.0,
        ),
        ("function eval() { return 4; } eval()", 4.0),
        ("var arguments = 2; arguments", 2.0),
        (
            "function f() { let arguments = 5; return () => arguments; } f()()",
            5.0,
        ),
        ("let arguments = 6; (() => arguments)()", 6.0),
        (
            "function f() { function arguments() { return 7; } return arguments(); } f()",
            7.0,
        ),
        (
            "function f() { { let arguments = 8; return arguments; } } f()",
            8.0,
        ),
        ("try { throw 9; } catch (eval) { eval }", 9.0),
    ] {
        assert_eq!(sloppy(src), ValueView::Number(expected), "{src}");
    }
    assert_eq!(
        sloppy("typeof arguments === 'undefined'"),
        ValueView::Bool(true)
    );
    // Where they would denote the global `eval` (clj-quickjs-106) or a
    // function's own `arguments` object (clj-quickjs-97), they are not
    // supported yet; `var arguments` in a function is that object.
    for (src, column) in [
        ("x = eval;", 5),
        ("function f() { return arguments; }", 23),
        ("function f() { var arguments = 2; return arguments; }", 42),
        (
            "let arguments = 1; function f() { return () => arguments; }",
            48,
        ),
        ("function arguments() { return arguments; }", 31),
    ] {
        let e = compile_script_with("sloppy", src, ScriptOptions::SLOPPY).unwrap_err();
        assert_eq!(
            (e.kind, e.line, e.column),
            (SyntaxErrorKind::Unsupported, 1, column),
            "{src}: {e}"
        );
    }

    // The entry points without options stay strict throughout.
    let script = exe("function f() { return () => 1; } f()()");
    let f = script.function(0).unwrap();
    assert!(script.is_strict() && f.is_strict() && f.function(0).unwrap().is_strict());
    assert!(compile_function("function f() {}").unwrap().is_strict());
    for src in ["function f(eval) {}", "let arguments = 1;"] {
        let e = compile_script("strict", src).unwrap_err();
        assert_eq!(e.kind, SyntaxErrorKind::ReservedWord, "{src}");
    }
    let e = compile_function("function f(eval) {}").unwrap_err();
    assert_eq!(e.kind, SyntaxErrorKind::ReservedWord);
}

/// Legacy octal and leading-zero decimal literals run in sloppy scripts and
/// are early errors in strict code (clj-quickjs-51).
#[test]
fn legacy_octal_literals_in_sloppy_scripts() {
    let src = "function f() { return 010; } f() + 017 + 08 + 09.5";
    let script =
        compile_script_with("sloppy", src, ScriptOptions::SLOPPY).unwrap_or_else(|e| panic!("{e}"));
    match run(&script, &[]) {
        RunOutcome::Complete(v) => assert_eq!(v.view(), ValueView::Number(40.5)),
        other => panic!("expected Complete, got {other:?}"),
    }
    for (src, options) in [
        (src, ScriptOptions::STRICT),
        (
            "function f() { 'use strict'; return 010; }",
            ScriptOptions::SLOPPY,
        ),
    ] {
        let e = compile_script_with("strict", src, options).unwrap_err();
        assert_eq!(e.kind, SyntaxErrorKind::InvalidToken, "{src}");
    }
}

/// Legacy octal and `\8` `\9` string escapes run in sloppy scripts and are
/// early errors in strict code (clj-quickjs-258).
#[test]
fn legacy_octal_escapes_in_sloppy_scripts() {
    let src = r#"function f() { return "\101\8"; } f() + "\400" === "A8 0""#;
    let script =
        compile_script_with("sloppy", src, ScriptOptions::SLOPPY).unwrap_or_else(|e| panic!("{e}"));
    match run(&script, &[]) {
        RunOutcome::Complete(v) => assert_eq!(v.view(), ValueView::Bool(true)),
        other => panic!("expected Complete, got {other:?}"),
    }
    for (src, options) in [
        (src, ScriptOptions::STRICT),
        (
            r#"function f() { 'use strict'; return "\101"; }"#,
            ScriptOptions::SLOPPY,
        ),
        (r#"'\9'; 'use strict';"#, ScriptOptions::SLOPPY),
    ] {
        let e = compile_script_with("strict", src, options).unwrap_err();
        assert_eq!(e.kind, SyntaxErrorKind::InvalidToken, "{src}");
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
    // (`let` for `var`; since clj-quickjs-69 the upstream `typeof õ` itself,
    // plus a plain read of `õ`, which is a ReferenceError); see
    // `docs/architecture.md`, "QuickJS references and provenance".
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
    assert_eq!(string("let Ãµ = 3; typeof õ"), "undefined");
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

/// Run-time failure positions follow the operator token (clj-quickjs-52):
/// a binary operator at the operator, a call at its `(` (also as a caller
/// frame's call site), references at the identifier. The scripts adapt
/// cases of `test_line_column_numbers` (line 1118) of
/// `research/quickjs/tests/test_builtin.js`, revision 7d1c13c5d8f8, where
/// `@` marks the expected position. Modified: a methodless object
/// stands in for `poisoned_number` (conversion throws TypeError),
/// `null` is called through a binding, and the caller-frame
/// case calls a script function instead of `Error`. The other scripts are
/// written for this project.
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
fn runtime_error_positions_follow_operator_tokens() {
    // Binary operators: at the operator, not the start of the expression.
    let (kind, _, line, column) = failure_at("let f = Object.create(null);\n  1 + 2 * f;");
    assert_eq!((kind, line, column), (VmErrorKind::TypeError, 2, 9));
    let (kind, _, line, column) = failure_at("let f = Object.create(null);\n  1 + 2 ** f;");
    assert_eq!((kind, line, column), (VmErrorKind::TypeError, 2, 9));
    let (kind, _, line, column) = failure_at("let f = Object.create(null);\n  f < 2");
    assert_eq!((kind, line, column), (VmErrorKind::TypeError, 2, 5));
    // Unary and update operators (unchanged): at the operator.
    let (_, _, line, column) = failure_at("let f = Object.create(null);\n  2 * - f");
    assert_eq!((line, column), (2, 7));
    let (_, _, line, column) = failure_at("let f = Object.create(null);\n  2 * f ++");
    assert_eq!((line, column), (2, 9));
    // Calls: at the `(`.
    let (kind, message, line, column) = failure_at("let n = null;\n 1 + n ( 1234 );");
    assert_eq!(
        (kind, message.as_str(), line, column),
        (VmErrorKind::TypeError, "null is not a function", 2, 8)
    );
    // Member reads and writes (clj-quickjs-114): at the `.`.
    let (kind, message, line, column) = failure_at("\n 1 + null . abcd;");
    assert_eq!(
        (kind, message.as_str(), line, column),
        (
            VmErrorKind::TypeError,
            "cannot read property 'abcd' of null",
            2,
            11
        )
    );
    let (kind, _, line, column) = failure_at("let obj;\n obj . a = 1;");
    assert_eq!((kind, line, column), (VmErrorKind::TypeError, 2, 6));
    // Intrinsic calls too.
    let (_, _, line, column) = failure_at("let v = 1;\n  __rt.nth ( v, 0 );");
    assert_eq!((line, column), (2, 12));
    let (kind, _, line, column) = failure_at("\n  __rt.effect ( 0, 1 );");
    assert_eq!((kind, line, column), (VmErrorKind::HostEffect, 2, 15));
    // A caller frame's call site is its call's `(`.
    let e =
        failure("function g() { return missing; }\nfunction f() {\n  return 1 + g ( );\n}\nf();");
    assert_eq!(e.kind(), VmErrorKind::ReferenceError);
    let d = e.diagnostic();
    let at = d.source.unwrap();
    assert_eq!((at.line, at.column), (1, 23));
    let sites: Vec<_> = d
        .call_chain
        .iter()
        .map(|c| {
            let at = c.source.unwrap();
            (at.line, at.column)
        })
        .collect();
    assert_eq!(sites, [(3, 16), (5, 2)]);
    // References stay at the identifier.
    let (kind, _, line, column) = failure_at("\n  1 + not_def");
    assert_eq!((kind, line, column), (VmErrorKind::ReferenceError, 2, 7));
    let (kind, _, line, column) = failure_at("1 + (not_def = 1)");
    assert_eq!((kind, line, column), (VmErrorKind::ReferenceError, 1, 6));
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

/// `var` (clj-quickjs-94): one binding per name per function or script,
/// hoisted to its entry and `undefined` there (no temporal dead zone).
#[test]
fn var_declarations_hoist_to_the_function() {
    // Read before the declaration: `undefined`, not a ReferenceError.
    assert_eq!(
        eval("function f() { const r = x; var x = 2; return r; } f()"),
        ValueView::Undefined
    );
    assert_eq!(eval("x; var x = 1;"), ValueView::Undefined);
    assert_eq!(num("x = 3; var x; x"), 3.0);
    // A `var` in a nested block (or any statement position) binds in the
    // function.
    assert_eq!(num("function f() { { var x = 1; } return x; } f()"), 1.0);
    assert_eq!(
        num(
            "function f(b) { if (b) var y = 1; else { var z = 2; } return y === undefined ? z : y; } f(false) * 10 + f(true)"
        ),
        21.0
    );
    assert_eq!(
        num(
            "function f() { try { var a = 1; throw 0; } catch (e) { var b = 2; } finally { var c = 3; } \
             switch (a) { case 1: var d = 4; } L: var g = 5; return a + b + c + d + g; } f()"
        ),
        15.0
    );
    // Loop bodies: an unexecuted `for` body still declares; an executed
    // one leaves its value; a `do` body's `var` is readable before the loop.
    assert_eq!(
        eval("function f() { for (; false;) var x = 1; return x; } f()"),
        ValueView::Undefined
    );
    assert_eq!(
        num("function f() { for (let i = 0; i < 3; i++) { var last = i; } return last; } f()"),
        2.0
    );
    assert_eq!(
        num(
            "function f() { const before = d; do var d = 7; while (false); return before === undefined ? d : -1; } f()"
        ),
        7.0
    );
    // A `var` named like a parameter is that parameter's binding.
    assert_eq!(num("function f(a) { var a; return a; } f(5)"), 5.0);
    assert_eq!(num("function f(a) { var a = 2; return a; } f(5)"), 2.0);
    // `var x;` does not reset an assigned value; repeating `var` is fine.
    assert_eq!(num("var v = 1; var v; v"), 1.0);
    assert_eq!(num("var v = 1; { var v = v + 1; } var v = v * 10; v"), 20.0);
    // A `var` statement has no completion value.
    assert_eq!(num("1; var w = 2;"), 1.0);
    // A closure created before the `var` statement sees its later value.
    assert_eq!(
        num("function f() { var g = function () { return x; }; var x = 3; return g(); } f()"),
        3.0
    );
    assert_eq!(
        num("var n = 0; function inc() { n = n + 1; } inc(); inc(); n"),
        2.0
    );
    // Closures made in successive iterations of a loop body share its one
    // `var` binding and all see its final value (with `let`, each
    // iteration has its own: `js_functions.rs`
    // `loop_iterations_allocate_distinct_captured_cells`).
    let src = "let i = 0;
               let first, last;
               while (i < 2) {
                 var j = i;
                 const get = function () { return j; };
                 if (i === 0) { first = get; }
                 last = get;
                 i = i + 1;
               }
               first() * 100 + last()";
    assert_eq!(num(src), 101.0);
    let src = "let i = 0;
               let first, last;
               while (i < 2) {
                 var j = i;
                 const bump = function () { j = j + 1; return j; };
                 if (i === 0) { first = bump; }
                 last = bump;
                 i = i + 1;
               }
               first() * 100 + last()";
    assert_eq!(num(src), 203.0);
    // A never-assigned `var` captured by a closure holds `undefined`.
    assert_eq!(
        eval("function f() { function g() { return u; } var u; return g(); } f()"),
        ValueView::Undefined
    );
    // A `var` of a function is not visible outside it.
    let (kind, message, ..) = failure_at("function f() { var inner = 1; } f(); inner");
    assert_eq!(
        (kind, message.as_str()),
        (VmErrorKind::ReferenceError, "inner is not defined")
    );
    // A `var` in a catch block named like its parameter (ECMAScript Annex
    // B) is a function binding, but its initializer assigns the parameter.
    assert_eq!(
        num(
            "function f() { try { throw 1; } catch (e) { var e = 2; var r = e; } return e === undefined ? r : -1; } f()"
        ),
        2.0
    );
    // A `var` shadows an enclosing function's binding of the same name.
    assert_eq!(
        num("let s = 1; function f() { s = 2; var s; return s; } f() * 10 + s"),
        21.0
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
    // (Functions and calls execute since clj-quickjs-21, property reads
    // since clj-quickjs-114, method calls since clj-quickjs-117; assigning
    // a built-in global waits for the global object, clj-quickjs-104.)
    assert_eq!(compile_err("let o = 1;\nObject = o;"), (Unsupported, 2, 1));
    // Syntax errors pass through from the parser.
    assert_eq!(compile_err("let if = 1;").0, SyntaxErrorKind::ReservedWord);
}

/// The contents of a script's string completion value.
fn string(src: &str) -> String {
    match run(&exe(src), &[]) {
        RunOutcome::Complete(v) => {
            assert_eq!(v.view(), ValueView::String, "{src:?}");
            v.string().expect("a string value").to_string()
        }
        other => panic!("{src:?}: expected Complete, got {other:?}"),
    }
}

#[test]
fn string_values() {
    // Literals compile to constants and complete with their contents.
    assert_eq!(string(r#""abc""#), "abc");
    assert_eq!(string(r"'it\'s \u{1F600}'"), "it's \u{1F600}");
    assert_eq!(string("''"), "");
    // Equality compares contents; a string never equals a non-string.
    assert_eq!(eval(r#""ab" === "a" + "b""#), ValueView::Bool(true));
    assert_eq!(eval(r#""a" !== "b""#), ValueView::Bool(true));
    assert_eq!(eval(r#""a" === 1"#), ValueView::Bool(false));
    assert_eq!(eval(r#""1" == "01""#), ValueView::Bool(false));
    assert_eq!(eval(r#""a" == "a""#), ValueView::Bool(true));
    assert_eq!(eval(r#""a" != "a""#), ValueView::Bool(false));
    assert_eq!(eval(r#""a" == null"#), ValueView::Bool(false));
    // ToBoolean is false only for the empty string.
    assert_eq!(num(r#""" ? 1 : 2"#), 2.0);
    assert_eq!(num(r#""0" ? 1 : 2"#), 1.0);
    assert_eq!(eval(r#"!"""#), ValueView::Bool(true));
    // `+` with two string operands concatenates; numbers still add.
    assert_eq!(string(r#"let s = "x"; s + s + s"#), "xxx");
    assert_eq!(string(r#"let f = (a, b) => a + b; f("p", "q")"#), "pq");
    assert_eq!(num("let f = (a, b) => a + b; f(1, 2)"), 3.0);
    assert_eq!(num("let t = true; t + 1"), 2.0);
    // Persistent map keys compare by content.
    assert_eq!(num(r#"__rt.get(__rt.map("k", 1), "k")"#), 1.0);
    assert_eq!(
        num(r#"let k = "k"; __rt.get(__rt.map("k" + "", 1, k + k, 2), "kk")"#),
        2.0
    );
    // Calling a string is a TypeError.
    let (kind, message, ..) = failure_at(r#""s"()"#);
    assert_eq!(kind, VmErrorKind::TypeError);
    assert_eq!(message, "string is not a function");
    // Functions convert using the available Object.prototype defaults.
    assert_eq!(string("'a' + (() => 1)"), "a[object Function]");
    assert_eq!(string("(() => 1) + 1"), "[object Function]1");
    // An uncaught thrown string is reported with its text.
    let e = failure(r#"throw "boom""#);
    assert_eq!(e.kind(), VmErrorKind::Thrown);
    assert!(e.message().contains("boom"), "{}", e.message());
    // A caught string is the same value.
    assert_eq!(
        eval(r#"let r; try { throw "e" + "x"; } catch (v) { r = v === "ex"; } r"#),
        ValueView::Bool(true)
    );
}

/// Binary `+` with a string operand concatenates ToString of both operands
/// (clj-quickjs-66). Two rows are adapted from
/// `research/quickjs/tests/test_language.js`, revision 7d1c13c5d8f8: the
/// message expression of the shared `assert` helper (line 1), with
/// `let` bindings for its parameters and without the optional message
/// part, and the number of `test_cvt` (line 108) whose
/// `.toString()` is `"19686109595169230000"`, converted here by `"" +`
/// (method calls on primitives are a later pellet); see
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
fn addition_concatenates_strings() {
    // When either operand is a string, `+` concatenates the ToString of
    // both, left to right.
    for (src, expected) in [
        (r#""a" + 1"#, "a1"),
        (r#"1 + "a""#, "1a"),
        (r#"1 + 2 + "3""#, "33"),
        (r#""3" + 1 + 2"#, "312"),
        (r#""" + true"#, "true"),
        (r#"false + """#, "false"),
        (r#""" + null"#, "null"),
        (r#""" + undefined"#, "undefined"),
        (r#""" + -0"#, "0"),
        // `test_cvt`.
        (r#""" + 19686109595169230000"#, "19686109595169230000"),
        (r#""" + 1e21"#, "1e+21"),
        (r#""" + 1.5e-7"#, "1.5e-7"),
        (r#""" + 0.000001"#, "0.000001"),
        (r#""" + 0.1"#, "0.1"),
        // An exact decimal tie takes the even last digit.
        (r#""" + 1000000000000000.25"#, "1000000000000000.2"),
        (r#""" + -1000000000000000.25"#, "-1000000000000000.2"),
        (r#""" + (0.1 + 0.2)"#, "0.30000000000000004"),
        (r#""" + NaN"#, "NaN"),
        (r#""" + -Infinity"#, "-Infinity"),
        // Operands that are not statically known still convert at run time.
        (
            r#"let f = (a, b) => a + b; f(2, "x") + f(null, "")"#,
            "2xnull",
        ),
        // The shared `assert` helper's message.
        (
            r#"let actual = 1, expected = true;
               "assertion failed: got |" + actual + "|" + ", expected |" + expected + "|""#,
            "assertion failed: got |1|, expected |true|",
        ),
    ] {
        assert_eq!(string(src), expected, "{src}");
    }
    // Without a string operand, `+` stays numeric.
    assert_eq!(num("1 + 2"), 3.0);
    assert_eq!(num("true + 1"), 2.0);
    assert_eq!(num("let n = null, t = true; n + t"), 1.0);
    assert!(num("undefined + 1").is_nan());
    // Both operands are evaluated before the conversion: an unresolvable
    // right operand fails with its ReferenceError.
    let (kind, message, ..) = failure_at(r#""x" + nope"#);
    assert_eq!(kind, VmErrorKind::ReferenceError);
    assert_eq!(message, "nope is not defined");
}

/// Strings convert to numbers with StringToNumber wherever ToNumber applies
/// (clj-quickjs-67). The four string groups of `test_cvt` (line 108) are
/// adapted from `research/quickjs/tests/test_language.js`, revision
/// 7d1c13c5d8f8 (each `assert(cond)` as a Rust assertion on the
/// expression's value); see `docs/architecture.md`, "QuickJS references and
/// provenance".
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
fn strings_convert_with_string_to_number() {
    for (src, expected) in [
        (r#"+"12345""#, 12345.0),
        (r#"+"""#, 0.0),
        (r#"+"  \n12\t""#, 12.0),
        (r#"+" 1﻿""#, 1.0),
        // A literal BOM in the source text.
        ("+\" 1\u{FEFF}\"", 1.0),
        (r#"+"  　 7 \r""#, 7.0),
        (r#"+"0x12345""#, 74565.0),
        (r#"+"0X1f""#, 31.0),
        (r#"+"0b11""#, 3.0),
        (r#"+"0o17""#, 15.0),
        (r#"+"1e3""#, 1000.0),
        (r#"+".5""#, 0.5),
        (r#"+"5.""#, 5.0),
        (r#"+"Infinity""#, f64::INFINITY),
        (r#"+"+Infinity""#, f64::INFINITY),
        (r#"+"-Infinity""#, f64::NEG_INFINITY),
        (r#"+"010""#, 10.0),
        (r#""6" * "7""#, 42.0),
        (r#""3" - 1"#, 2.0),
        (r#"-"2""#, -2.0),
        (r#""7" / "2""#, 3.5),
        (r#""7" % "4""#, 3.0),
        (r#""2" ** "10""#, 1024.0),
        (r#"~"5""#, -6.0),
        // `++`/`--` convert first: the binding holds a number afterwards.
        (r#"let s = "41"; s++; s"#, 42.0),
        (r#"let s = " 8 "; --s"#, 7.0),
        // Operands that are not statically known convert at run time.
        (r#"let f = (a, b) => a * b; f("3", " 4 ")"#, 12.0),
        // `test_cvt`.
        (r#""12345" | 0"#, 12345.0),
        (r#""0x12345" | 0"#, 74565.0),
        (r#""12345" >>> 0"#, 12345.0),
        (r#""0x12345" >>> 0"#, 74565.0),
        (r#""-1" >>> 0"#, 4294967295.0),
        (r#"1 << "31""#, -2147483648.0),
    ] {
        assert_eq!(num(src), expected, "{src}");
    }
    // `test_cvt`, as written there.
    for src in [
        r#"("12345" | 0) === 12345"#,
        r#"("0x12345" | 0) === 0x12345"#,
        r#"("12345" >>> 0) === 12345"#,
        r#"("0x12345" >>> 0) === 0x12345"#,
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
    // The `docs/grammar.md` example lines, each true.
    for src in [
        r#"+" 12\n" === 12 && +"0x1f" === 31 && +"" === 0"#,
        r#"("12345" | 0) === 12345 && ("0x12345" >>> 0) === 0x12345"#,
        r#""6" * "7" === 42 && "2" ** "10" === 1024"#,
        r#""" == 0 && "123" == 123 && "1" == true && "abc" != NaN"#,
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
    let minus_zero = num(r#"+"-0""#);
    assert!(minus_zero == 0.0 && minus_zero.is_sign_negative());
    for src in [
        r#"+"-0x1""#,
        r#"+"infinity""#,
        r#"+"1_000""#,
        r#"+"1n""#,
        r#"+"12px""#,
        r#"+"0x""#,
        r#"+".""#,
        r#"+"1e""#,
        r#"+"--1""#,
        r#""abc" * 1"#,
        r#"let a = "a"; a - 1"#,
        r#"let s = "x"; s++"#,
    ] {
        assert!(num(src).is_nan(), "{src}");
    }
    // Relational comparison converts a string compared with a non-string.
    for (src, expected) in [
        (r#""10" < 9"#, false),
        (r#"9 < "10""#, true),
        (r#""abc" < 1"#, false),
        (r#""abc" >= 1"#, false),
        (r#"let t = true; "0.5" < t"#, true),
    ] {
        assert_eq!(eval(src), ValueView::Bool(expected), "{src}");
    }
    // Two strings compare by code units (clj-quickjs-68,
    // `strings_compare_by_code_units`), never as numbers.
    for (src, expected) in [
        (r#""10" < "9""#, true),
        (r#""9" <= "10""#, false),
        (r#"let a = "1", b = "2"; a >= b"#, false),
        (r#"let f = (a, b) => a > b; f("b", "a")"#, true),
    ] {
        assert_eq!(eval(src), ValueView::Bool(expected), "{src}");
    }
}

/// Relational comparison of two strings by UTF-16 code units
/// (clj-quickjs-68). `'b' > 'a'` is from `test_op1` (the case on line 103) of
/// `research/quickjs/tests/test_language.js`, revision 7d1c13c5d8f8 (its
/// `assert(a, b, msg)` as a Rust assertion on the expression's value); see
/// `docs/architecture.md`, "QuickJS references and provenance". The QuickJS
/// MIT notice is on `strings_convert_with_string_to_number` above.
#[test]
fn strings_compare_by_code_units() {
    for (src, expected) in [
        // `test_op1`.
        ("'b' > 'a'", true),
        ("'a' < 'b'", true),
        // A proper prefix is smaller; the empty string is the smallest.
        ("'a' < 'aa'", true),
        ("'aa' > 'a'", true),
        ("'' < 'a'", true),
        ("'' < ''", false),
        ("'' <= ''", true),
        // Code units, not case-insensitive: 'B' is 0x42, 'a' is 0x61.
        ("'B' < 'a'", true),
        ("'a' <= 'a'", true),
        ("'a' >= 'a'", true),
        ("'a' > 'a'", false),
        ("'a' >= 'b'", false),
        ("'abd' > 'abc'", true),
        // Two strings compare as strings, a string and a number as numbers.
        ("'10' < '9'", true),
        ("'10' < 9", false),
        ("9 > '10'", false),
        // 'abc' converts to NaN: every comparison is false.
        ("'abc' < 1", false),
        ("'abc' >= 1", false),
        ("1 <= 'abc'", false),
        // Code-unit order: U+10000 is D800 DC00, below U+FFFF (FFFF) even
        // though its scalar value (and UTF-8 encoding) is larger.
        ("'\\u{10000}' < '\u{FFFF}'", true),
        ("'\u{FFFF}' < '\\u{10000}'", false),
        ("'\\u{10000}' > '\\uE000'", false),
        ("'\\u{10000}' > '\\uD7FF'", true),
        ("'\\u{10000}' < '\\u{10001}'", true),
        // Strings that are not statically known compare at run time, in
        // all four operators.
        (
            "let f = (a, b) => (a < b) + 2 * (a > b) + 4 * (a <= b) + 8 * (a >= b); \
             f('x', 'y') === 5 && f('y', 'x') === 10 && f('x', 'x') === 12",
            true,
        ),
        // Booleans, null and undefined convert with ToNumber.
        ("let t = true, s = '1'; t <= s && t >= s", true),
        (
            "let n = null, u; n < '1' && !(u < '1') && !(u >= '1')",
            true,
        ),
    ] {
        assert_eq!(eval(src), ValueView::Bool(expected), "{src}");
    }
    // The `docs/grammar.md` example lines, each true.
    for src in [
        r#""b" > "a" && "a" < "aa" && "" < "a" && "B" < "a" && "a" <= "a""#,
        r#""10" < "9" && !("10" < 9) && !("abc" < 1) && !("abc" >= 1)"#,
        r#""\u{10000}" < "\uFFFF" && !("\uFFFF" < "\u{10000}")"#,
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
    // The relational operators with no static number operand lower to the
    // string-aware compare opcodes; with one, to ToNumber and `lt`.
    let ops = opcodes(&exe("let a = 'a', b = 'b'; a < b; a > b; a <= b; a >= b"));
    for op in [
        Opcode::CompareLt,
        Opcode::CompareGt,
        Opcode::CompareLe,
        Opcode::CompareGe,
    ] {
        assert!(ops.contains(&op), "{op:?} missing from {ops:?}");
    }
    let ops = opcodes(&exe("let a = 'a'; a < 1"));
    assert!(
        ops.contains(&Opcode::Lt) && !ops.contains(&Opcode::CompareLt),
        "{ops:?}"
    );
    // Functions use the available Object.prototype default, which is nonnumeric.
    assert_eq!(
        eval("let o = function () {}; 'a' < o"),
        ValueView::Bool(false)
    );
}

/// Strings are sequences of UTF-16 code units, lone surrogates included
/// (clj-quickjs-141). The literal rows of the unicode group of
/// `test_string` (line 235) and its `"\0"` row (line 339) are adapted from
/// `research/quickjs/tests/test_builtin.js`, revision 7d1c13c5d8f8: each
/// `assert(a, b)` is a Rust assertion on `a === b`, the string that upstream
/// builds with `String.fromCharCode(0x20ac)` is the literal `"€"`, and
/// `eval('"\0"')` is the literal itself (`length`, `charCodeAt`,
/// `codePointAt`, `String.fromCharCode` / `String.fromCodePoint` and `eval`
/// are later pellets); see `docs/architecture.md`, "QuickJS references and
/// provenance". The QuickJS MIT notice is on
/// `strings_convert_with_string_to_number` above.
#[test]
fn strings_are_code_unit_sequences() {
    for (src, expected) in [
        // `test_string`: U+10FFFF is the pair DBFF DFFF, whether written as
        // the character, a braced escape or two braced surrogate escapes.
        ("\"\\u{10ffff}\" === \"\\u{dbff}\\u{dfff}\"", true),
        ("\"\u{10FFFF}\" === \"\\u{10ffff}\"", true),
        ("\"\u{20AC}\" === \"\u{20AC}\"", true),
        ("\"\u{20AC}\" === \"\\u20ac\"", true),
        ("\"\u{20AC}\" === \"\\u{20ac}\"", true),
        ("\"a\" === \"\\x61\"", true),
        ("\"\\0\" === \"\\u0000\"", true),
        ("\"\\0\" !== \"\"", true),
        // Halves joined by concatenation form the pair, statically known
        // or not.
        ("\"\\u{dbff}\" + \"\\u{dfff}\" === \"\\u{10ffff}\"", true),
        (
            "let h = \"\\uDBFF\", l = \"\\uDFFF\"; h + l === \"\u{10FFFF}\"",
            true,
        ),
        (
            "let h = \"\\uD83D\"; h + \"\\uDE00!\" === \"\u{1F600}!\"",
            true,
        ),
        // A lone surrogate is its own value: not its partner, not U+FFFD.
        ("\"\\u{dbff}\" === \"\\u{dfff}\"", false),
        ("\"\\uD800\" === \"\u{FFFD}\"", false),
        ("\"\\uD800\" === \"\\u{d800}\"", true),
        ("\"\\uD800\" == \"\\uD800\"", true),
        ("\"\\uD800\" ? true : false", true),
        // Relational order is by code units.
        ("\"\\uD800\" < \"\\uDC00\"", true),
        ("\"\\uDC00\\uD800\" > \"\\uD800\\uDC00\"", true),
        ("\"\\uDBFF\" < \"\\u{10ffff}\"", true),
        ("\"\\u{10ffff}\" < \"\\uE000\"", true),
        // Persistent map keys compare code units.
        (
            "__rt.get(__rt.map(\"\\u{10ffff}\", 1), \"\\u{dbff}\\u{dfff}\") === 1",
            true,
        ),
        (
            "let m = __rt.map(\"\\uD800\", 1, \"\u{FFFD}\", 2); \
             __rt.get(m, \"\\u{d800}\") === 1 && __rt.get(m, \"\\uFFFD\") === 2",
            true,
        ),
    ] {
        assert_eq!(eval(src), ValueView::Bool(expected), "{src}");
    }

    // An embedder reads a string result with a lone surrogate exactly
    // (code units, `Debug`) or lossily (`Display`), without panicking.
    let rt = Runtime::new();
    let script = exe("let s = \"a\\uD800\"; s");
    let RunOutcome::Complete(lone) = rt.spawn(&script, &[]).unwrap().run().unwrap() else {
        panic!("expected Complete");
    };
    let s = lone.string().expect("a string value");
    assert_eq!(s.code_units(), [0x61, 0xD800]);
    assert_eq!(format!("{s:?}"), "\"a\\u{d800}\"");
    assert_eq!(s.to_string(), "a\u{FFFD}");

    // Published to another task on another thread, it compares equal by
    // content there and joins with a low surrogate into a pair.
    assert_eq!(rt.publish(&lone), Ok(()));
    let consumer = compile_function(
        "function consume(s) {
           return s === \"a\\u{d800}\" && s !== \"a\u{FFFD}\" &&
                  s + \"\\uDC00\" === \"a\u{10000}\" && __rt.get(__rt.map(s, 7), \"a\\uD800\");
         }",
    )
    .unwrap();
    let task = rt.spawn(&consumer, &[&lone]).unwrap();
    let outcome = std::thread::scope(|t| t.spawn(|| task.run().unwrap()).join().unwrap());
    let RunOutcome::Complete(v) = outcome else {
        panic!("expected Complete, got {outcome:?}");
    };
    assert_eq!(v.view(), ValueView::Number(7.0));
}

/// IsLooselyEqual between strings and numbers or booleans (clj-quickjs-67).
/// The string rows of `test_eq` (line 126) are adapted from
/// `research/quickjs/tests/test_language.js`, revision 7d1c13c5d8f8 (each
/// `assert(cond)` as a Rust assertion on the expression's value); see
/// `docs/architecture.md`, "QuickJS references and provenance". The QuickJS
/// MIT notice is on `strings_convert_with_string_to_number` above.
#[test]
fn strings_compare_loosely_with_numbers() {
    for (src, expected) in [
        // `test_eq`.
        (r#""" == 0"#, true),
        (r#""123" == 123"#, true),
        (r#""122" != 123"#, true),
        // A number against a string compares with StringToNumber.
        (r#"123 == "123""#, true),
        (r#""abc" == NaN"#, false),
        (r#""NaN" == NaN"#, false),
        (r#"" 1 " == 1"#, true),
        (r#""1e3" == 1000"#, true),
        (r#""0x10" == 16"#, true),
        (r#""-0" == 0"#, true),
        (r#""1" != 1"#, false),
        // A boolean converts to a number first.
        (r#""1" == true"#, true),
        (r#""0" == false"#, true),
        (r#""2" == true"#, false),
        (r#"true == "1""#, true),
        (r#"false == """#, true),
        (r#"false != " ""#, false),
        // A string never equals `null` or `undefined`.
        (r#""" == null"#, false),
        (r#"undefined == """#, false),
        (r#""null" == null"#, false),
        // Two strings still compare by content, not as numbers.
        (r#""1" == "01""#, false),
        // Operands that are not statically known compare at run time.
        (r#"let f = (a, b) => a == b; f("5", 5) && f(5, "5")"#, true),
    ] {
        assert_eq!(eval(src), ValueView::Bool(expected), "{src}");
    }
}

/// The `typeof` operator (clj-quickjs-69). The `typeof 1`, `typeof null` and
/// `typeof unknown_var` rows are adapted from `test_op2` (line 195) of
/// `research/quickjs/tests/test_language.js`, revision 7d1c13c5d8f8 (each
/// `assert(actual, expected)` as a Rust assertion); `typeof Object` waits
/// for a global `Object` (clj-quickjs-77). The `typeof õ` case of
/// `test_unicode_ident` is in `identifiers_are_not_normalized`, whose
/// QuickJS MIT notice covers these rows too; see `docs/architecture.md`,
/// "QuickJS references and provenance".
#[test]
fn typeof_operator() {
    for (src, expected) in [
        // `test_op2`.
        ("typeof 1", "number"),
        ("typeof null", "object"),
        ("typeof unknown_var", "undefined"),
        // The pellet's acceptance cases.
        ("typeof NaN", "number"),
        ("typeof undefined", "undefined"),
        ("typeof (unknown_var)", "undefined"),
        ("typeof true", "boolean"),
        (r#"typeof "s""#, "string"),
        ("typeof typeof 1", "string"),
        ("typeof function () {}", "function"),
        ("function f() {} typeof f", "function"),
        ("typeof __rt.vector()", "object"),
        ("typeof __rt.vectorBuilder()", "object"),
        (r#"typeof -"1""#, "number"),
        // Every other value kind source code can make.
        ("typeof false", "boolean"),
        ("typeof -0", "number"),
        ("typeof ''", "string"),
        ("typeof (() => 1)", "function"),
        ("let k = 2; typeof (() => k)", "function"),
        ("function g() { return () => 1; } typeof g()", "function"),
        ("typeof __rt.map()", "object"),
        ("typeof __rt.mapBuilder()", "object"),
        ("typeof __rt.freeze(__rt.vectorBuilder())", "object"),
        (
            "let t; try { missing; } catch (e) { t = typeof e; } t",
            "object",
        ),
        (
            "let t; try { throw 'x'; } catch (e) { t = typeof e; } t",
            "string",
        ),
        // Bindings: the type of the current value.
        ("let u; typeof u", "undefined"),
        ("let n = null; typeof n", "object"),
        ("let a = 1; a = 'one'; typeof a", "string"),
        // An unresolvable name inside a function, through two sets of
        // parentheses and a comment, and in a closure.
        ("function h() { return typeof nope; } h()", "undefined"),
        ("typeof ( /* c */ (nope))", "undefined"),
        ("let c = () => typeof nope; c()", "undefined"),
        // The result is an ordinary string.
        ("typeof 1 + typeof 'a'", "numberstring"),
        ("typeof typeof unknown_var", "string"),
    ] {
        assert_eq!(string(src), expected, "{src}");
    }
    // The `docs/grammar.md` example lines, each true.
    for src in [
        r#"typeof 1 === "number" && typeof null === "object" && typeof typeof 1 === "string""#,
        r#"typeof nope === "undefined" && typeof (nope) === "undefined" && typeof undefined === "undefined""#,
        r#"typeof (() => 1) === "function" && typeof __rt.vector() === "object" && typeof "" === "string""#,
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
    // Only the operand of `typeof` is exempt: a plain read still throws.
    let (kind, _, _, column) = failure_at("typeof x == 'undefined' && !x");
    assert_eq!((kind, column), (VmErrorKind::ReferenceError, 29));
    // The operand is evaluated: its effects and failures happen.
    assert_eq!(num("let i = 0; typeof (i = i + 1); typeof i++; i"), 2.0);
    let (kind, message, line, column) = failure_at("typeof (0, nope)");
    assert_eq!(kind, VmErrorKind::ReferenceError);
    assert_eq!(message, "nope is not defined");
    assert_eq!((line, column), (1, 12));
    let (kind, ..) = failure_at("typeof nope()");
    assert_eq!(kind, VmErrorKind::ReferenceError);
    // A binding in its temporal dead zone still throws, statically ...
    let (kind, message, line, column) = failure_at("{ typeof x; let x; }");
    assert_eq!(kind, VmErrorKind::ReferenceError);
    assert_eq!(message, "cannot access 'x' before initialization");
    assert_eq!((line, column), (1, 10));
    let (kind, _, _, column) = failure_at("{ typeof (x); const x = 1; }");
    assert_eq!((kind, column), (VmErrorKind::ReferenceError, 11));
    // ... and through a closure that runs before the declaration.
    let (kind, message, line, _) = failure_at("let f = () => typeof y;\nf();\nlet y = 1;");
    assert_eq!(kind, VmErrorKind::ReferenceError);
    assert_eq!(message, "cannot access 'y' before initialization");
    assert_eq!(line, 1);
    assert_eq!(string("let f = () => typeof y; let y = 1; f()"), "number");
    // The reserved intrinsic binding is still not a value.
    let e = compile_script("t", "typeof __rt").unwrap_err();
    assert_eq!(e.kind, SyntaxErrorKind::ReservedWord);
    // `typeof` lowers to the `typeof` opcode; an unresolvable operand to
    // `undefined` before it, with no `throw_error`.
    let ops = opcodes(&exe("typeof unknown_var"));
    assert!(
        ops.contains(&Opcode::Typeof) && !ops.contains(&Opcode::ThrowError),
        "{ops:?}"
    );
}

#[test]
fn void_operator() {
    // The pellet's acceptance cases (clj-quickjs-133); `void 0` is the
    // `assert(x, void 0)` of `test_destructuring` in QuickJS's
    // `tests/test_language.js`.
    assert_eq!(eval("void 0"), ValueView::Undefined);
    assert_eq!(eval("void 0 === undefined"), ValueView::Bool(true));
    assert_eq!(eval("void void 1"), ValueView::Undefined);
    // The operand is evaluated exactly once, for its effects.
    let calls = "let n = 0; function f() { n = n + 1; return 7; } let r = void f();";
    assert_eq!(eval(&format!("{calls} r")), ValueView::Undefined);
    assert_eq!(num(&format!("{calls} n")), 1.0);
    assert_eq!(num("let i = 0; void i++; void (i = i + 10); i"), 11.0);
    // A throwing operand's exception reaches `catch`, and the `void`
    // expression produces no value.
    assert_eq!(
        string(
            "let t = 'none'; try { t = void (() => { throw 'boom'; })(); } catch (e) { t = e; } t"
        ),
        "boom"
    );
    assert_eq!(
        string("let t = 'none'; try { void nope; } catch (e) { t = typeof e; } t"),
        "object"
    );
    // Unlike `typeof`, an unresolvable name is read and fails.
    let (kind, message, line, column) = failure_at("void nope");
    assert_eq!(kind, VmErrorKind::ReferenceError);
    assert_eq!(message, "nope is not defined");
    assert_eq!((line, column), (1, 6));
    // Precedence of the other prefix operators; any value is discarded.
    assert_eq!(string("typeof void 0"), "undefined");
    assert!(num("void 1 + 1").is_nan());
    assert_eq!(eval("!void 0"), ValueView::Bool(true));
    assert_eq!(eval("void 'a' === void {}"), ValueView::Bool(true));
    assert_eq!(eval("1; void 2"), ValueView::Undefined);
    assert_eq!(eval("let g = () => void 3; g()"), ValueView::Undefined);
    // The `docs/grammar.md` example line, true.
    assert_eq!(
        eval(
            "void 0 === undefined && void void 1 === undefined && typeof void 0 === \"undefined\""
        ),
        ValueView::Bool(true)
    );
    // Lowered with existing opcodes: the operand, `pop`, `undefined`.
    let ops = opcodes(&exe("let n = 1; void n"));
    let at = ops.iter().position(|&op| op == Opcode::Pop).expect("pop");
    assert_eq!(ops[at + 1], Opcode::Undefined, "{ops:?}");
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
    // 240 additions, 240 comma operators and 240 (right-associative)
    // exponentiations. Compiling recurses once per tree
    // level, so it must fit the same 2 MiB thread stack the parser is tested
    // with.
    let nested = format!("{}1{}", "(".repeat(60), ")".repeat(60));
    let chain = vec!["1"; 241].join(" + ");
    let commas = vec!["1"; 241].join(", ");
    let powers = vec!["1"; 241].join(" ** ");
    // Objects (clj-quickjs-114): 60 nested literals, a chain of 240 member
    // reads and 250 (right-associative) property assignments.
    let objects = format!("x = {}1{};", "{a: 1, b: ".repeat(60), "}".repeat(60));
    let members = format!("o{};", ".b".repeat(240));
    let assigns = format!("{}1;", "o.b = ".repeat(250));
    // `void` (clj-quickjs-133): 240 nested prefix operators.
    let voids = format!("{}1;", "void ".repeat(240));
    let handle = std::thread::Builder::new()
        .stack_size(2 << 20)
        .spawn(move || {
            let a = compile_script("nested", &nested).map(|_| ());
            let b = compile_script("chain", &chain).map(|_| ());
            let c = compile_script("commas", &commas).map(|_| ());
            let d = compile_script("powers", &powers).map(|_| ());
            let e = compile_script("objects", &objects).map(|_| ());
            let f = compile_script("members", &members).map(|_| ());
            let g = compile_script("assigns", &assigns).map(|_| ());
            let h = compile_script("voids", &voids).map(|_| ());
            (a, b, c, d, e, f, g, h)
        })
        .unwrap();
    let (a, b, c, d, e, f, g, h) = handle.join().unwrap();
    a.unwrap();
    b.unwrap();
    c.unwrap();
    d.unwrap();
    e.unwrap();
    f.unwrap();
    g.unwrap();
    h.unwrap();
    assert_eq!(num(&vec!["1"; 241].join(" ** ")), 1.0);
    assert_eq!(num(&vec!["1"; 241].join(" + ")), 241.0);
    assert_eq!(
        num(&(1..=241)
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(", ")),
        241.0
    );
}

/// Right-associative chains of 240 compound assignments on properties
/// (clj-quickjs-260), named and computed, compile on a 2 MiB thread stack
/// (each level keeps the lowering's `expr` and `compound_assign_property`
/// frames while its right operand compiles; 240 is near the parser's
/// nesting limit, which these chains used to overflow before reaching,
/// clj-quickjs-262) and run.
#[test]
fn deep_property_compound_assignments_compile_within_a_small_stack() {
    let handle = std::thread::Builder::new()
        .stack_size(2 << 20)
        .spawn(|| {
            [
                ("o.b", "let o = {b: 0};"),
                ("o[0]", "let o = [0];"),
                ("o.b[k]", "let k = 'c'; let o = {b: {c: 0}};"),
            ]
            .map(|(target, init)| {
                // Each level adds the old value 0 to the result of the
                // level to its right, so the value is 1.
                let src = format!("{init} {}1", format!("{target} += ").repeat(240));
                compile_script("deep", &src)
                    .map(|exe| (target, exe))
                    .map_err(|e| format!("{target}: {e}"))
            })
        })
        .unwrap();
    for compiled in handle.join().unwrap() {
        let (target, exe) = compiled.unwrap();
        match run(&exe, &[]) {
            RunOutcome::Complete(v) => assert_eq!(v.view(), ValueView::Number(1.0), "{target}"),
            other => panic!("{target}: expected Complete, got {other:?}"),
        }
    }
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

/// Untagged template literals (clj-quickjs-57). `TEST_TEMPLATE` is adapted
/// from `test_template` (line 356) of `research/quickjs/tests/test_language.js`
/// (QuickJS, revision 7d1c13c5d8f8; see `docs/architecture.md` for the
/// attribution): the `String.raw` case (clj-quickjs-58) is dropped and each
/// `assert(x, v)` is a variable whose value the completion checks.
/// `TEST_TEMPLATE_SKIP` is adapted from `test_template_skip` (line 371):
/// the destructuring default `var { b = ... } = {}` (destructuring is not
/// supported) is a plain `var b = ...`. The other cases are written for
/// this project.
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
fn template_literal_values() {
    const TEST_TEMPLATE: &str = r#"
        var a, b;
        b = 123;
        a = `abc${b}d`;
        var first = a;
        a = "aaa";
        b = "bbb";
        first + "|" + `aaa${a, b}ccc`
    "#;
    const TEST_TEMPLATE_SKIP: &str = r#"
        var a = "Bar";
        var b = `${a + `a${a}` }baz`;
        b
    "#;
    assert_eq!(string(TEST_TEMPLATE), "abc123d|aaabbbccc");
    assert_eq!(string(TEST_TEMPLATE_SKIP), "BaraBarbaz");
    // The pellet's acceptance forms.
    assert_eq!(string("let b = 123; `abc${b}d`"), "abc123d");
    assert_eq!(
        string("let a = \"Bar\"; `${a + `a${a}` }baz`"),
        "BaraBarbaz"
    );
    assert_eq!(
        string("`${1}${true}${null}${undefined}`"),
        "1truenullundefined"
    );
    assert_eq!(
        string("let a = \"aaa\", b = \"bbb\"; `aaa${a, b}ccc`"),
        "aaabbbccc"
    );
    // ToString of numbers is Number::toString; a string stays itself.
    assert_eq!(
        string("`${1.5}|${-0}|${1e21}|${NaN}|${-Infinity}|${0.1 + 0.2}|${'s'}`"),
        "1.5|0|1e+21|NaN|-Infinity|0.30000000000000004|s"
    );
    // No substitutions, empty parts, a substitution alone.
    assert_eq!(string("``"), "");
    assert_eq!(string("`plain`"), "plain");
    assert_eq!(string("`${''}${''}`"), "");
    assert_eq!(string("`${42}`"), "42");
    assert_eq!(eval("typeof `${1}`"), eval("'string'"));
    // Escapes are decoded; raw line breaks are text, CRLF read as LF; a
    // line continuation is nothing.
    assert_eq!(
        string("`a\\tb\\x41\\u{1F600}\\`\\${x}\r\nc\\\nd`"),
        "a\tbA\u{1F600}`${x}\ncd"
    );
    // Substitutions run left to right, each converted before the next.
    assert_eq!(
        string(
            "let log = ''; function f(x) { log = log + x; return x; }
             `${f(1)}-${f(2)}-${f(3)}` + ':' + log"
        ),
        "1-2-3:123"
    );
    // Closures and nesting.
    assert_eq!(
        string("let x = 1; let g = () => `<${x}>`; x = 2; `${g()}${`${g()}`}`"),
        "<2><2>"
    );
    // A template is a string value like any other.
    assert_eq!(eval("`ab` === 'ab'"), ValueView::Bool(true));
    assert_eq!(num("`2` * `${3}`"), 6.0);
    // Object ToString completes before the next substitution throws.
    let (kind, _, line, column) =
        failure_at("let o = {};\nfunction g() { throw 'later'; }\n`a${o}${g()}`");
    assert_eq!((kind, line, column), (VmErrorKind::Thrown, 2, 16));
    // The conversion is ToString, an instruction of its own.
    assert!(opcodes(&exe("let b = 1; `a${b}`")).contains(&Opcode::ToString));
}

/// Templates nested in substitutions as deep as the parser allows compile
/// within the 2 MiB thread stack the parser is tested with, and run.
#[test]
fn deep_templates_compile_within_a_small_stack() {
    let nested = |n: usize| format!("{}1{}", "`<${".repeat(n), "}>`".repeat(n));
    let handle = std::thread::Builder::new()
        .stack_size(2 << 20)
        .spawn(move || {
            (1..100)
                .map(|n| (n, compile_script("templates", &nested(n))))
                .take_while(|(_, c)| !matches!(c, Err(e) if e.kind == SyntaxErrorKind::TooDeep))
                .last()
                .unwrap()
        })
        .unwrap();
    let (depth, exe) = handle.join().unwrap();
    assert!(depth >= 40, "only {depth} nested templates");
    match run(&exe.unwrap(), &[]) {
        RunOutcome::Complete(v) => assert_eq!(
            v.string().expect("a string").to_string(),
            format!("{}1{}", "<".repeat(depth), ">".repeat(depth))
        ),
        other => panic!("{other:?}"),
    }
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
/// operand cases of `test_cvt` are in `strings_convert_with_string_to_number`); see
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
    // Objects without conversion methods throw a catchable TypeError.
    for src in [
        "__rt.vector() | 0",
        "1 << __rt.vector()",
        "~__rt.vector()",
        "~Object.create(null)",
    ] {
        assert_eq!(failure(src).kind(), VmErrorKind::TypeError, "{src}");
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

/// The exponentiation operator `**` (clj-quickjs-63). `2 ** 8` is adapted
/// from `test_op1` in `research/quickjs/tests/test_language.js`, revision
/// 7d1c13c5d8f8 (`assert(2 ** 8, 256, ...)` as a Rust assertion); see
/// `docs/architecture.md`, "QuickJS references and provenance". The other
/// cases are written for this project from ECMAScript's
/// Number::exponentiate.
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
fn exponent_operator() {
    // `test_op1`.
    assert_eq!(num("2 ** 8"), 256.0);
    // Exact values, compared bit for bit (so `-0` and `+0` differ).
    for (src, expected) in [
        ("2 ** 3 ** 2", 512.0),
        ("(2 ** 3) ** 2", 64.0),
        ("(-2) ** 2", 4.0),
        ("(-2) ** 3", -8.0),
        ("2 ** -1", 0.5),
        ("-(2 ** 2)", -4.0),
        ("2 * 3 ** 2", 18.0),
        ("NaN ** 0", 1.0),
        ("NaN ** -0", 1.0),
        ("0 ** -1", f64::INFINITY),
        ("(-0) ** -1", f64::NEG_INFINITY),
        ("(-0) ** -2", f64::INFINITY),
        ("(-0) ** 2", 0.0),
        ("(-0) ** 3", -0.0),
        ("0 ** 0", 1.0),
        ("Infinity ** -1", 0.0),
        ("(-Infinity) ** 3", f64::NEG_INFINITY),
        ("(-Infinity) ** -3", -0.0),
        ("2 ** Infinity", f64::INFINITY),
        ("0.5 ** Infinity", 0.0),
        ("2 ** -Infinity", 0.0),
        ("(-1) ** 2", 1.0),
        ("1 ** 1e308", 1.0),
        // ToNumber of the other primitives.
        ("true ** 2", 1.0),
        ("null ** 0", 1.0),
        ("2 ** true", 2.0),
        ("2 ** null", 1.0),
        ("false ** false", 1.0),
    ] {
        let n = num(src);
        assert_eq!(n.to_bits(), expected.to_bits(), "{src} gave {n}");
    }
    // NaN, including where IEEE 754 `pow` gives 1: a non-finite exponent
    // with base 1 or -1.
    for src in [
        "(-8) ** (1 / 3)",
        "1 ** Infinity",
        "1 ** -Infinity",
        "1 ** NaN",
        "(-1) ** Infinity",
        "(-1) ** -Infinity",
        "(-1) ** NaN",
        "NaN ** 1",
        "2 ** NaN",
        "2 ** undefined",
    ] {
        assert!(num(src).is_nan(), "{src} gave {}", num(src));
    }
    assert_eq!(num("undefined ** 0"), 1.0);

    // Both operands are evaluated, left first, before any conversion.
    assert_eq!(failure_at("nope ** 1").0, VmErrorKind::ReferenceError);
    assert_eq!(failure_at("1 ** nope").0, VmErrorKind::ReferenceError);
    assert_eq!(
        failure_at("__rt.vector() ** nope").0,
        VmErrorKind::ReferenceError
    );
    assert_eq!(
        num("let x = 0; let y = (x = 2) ** (x = 3); y * 10 + x"),
        83.0
    );
    // Right-associative at run time with bindings, inside functions.
    assert_eq!(
        num("const sq = (x) => x ** 2; let b = 2; sq(b ** b ** b) / 2 ** -1"),
        512.0
    );
    // Objects without conversion methods throw a catchable TypeError.
    for src in [
        "__rt.vector() ** 2",
        "2 ** __rt.vector()",
        "Object.create(null) ** 0",
    ] {
        assert_eq!(failure(src).kind(), VmErrorKind::TypeError, "{src}");
    }
    // One VM instruction, with no ToNumber for number operands.
    let ops = opcodes(&exe("(2 ** -1) ** (1 ** ~2)"));
    assert_eq!(ops.iter().filter(|op| **op == Opcode::Pow).count(), 3);
    assert!(
        !ops.contains(&Opcode::ToNumber) && !ops.contains(&Opcode::ToNumberPair),
        "{ops:?}"
    );
    assert!(opcodes(&exe("let t = true; t ** 2")).contains(&Opcode::ToNumberPair));
    assert!(opcodes(&exe("let t = true; 2 ** t")).contains(&Opcode::ToNumber));
}

/// Prefix and postfix `++`/`--` on identifier bindings (clj-quickjs-64).
/// The first four cases are adapted from the identifier groups of
/// `test_inc_dec` in `research/quickjs/tests/test_language.js`, revision
/// 7d1c13c5d8f8 (`var a, r` becomes `let`, and `assert(r === 1 && a === 2,
/// true)` the script's value); see `docs/architecture.md`, "QuickJS
/// references and provenance". The other cases are written for this
/// project.
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
fn update_expressions_on_bindings() {
    // `test_inc_dec`, identifier groups.
    for src in [
        "let a, r; a = 1; r = a++; r === 1 && a === 2",
        "let a, r; a = 1; r = ++a; r === 2 && a === 2",
        "let a, r; a = 1; r = a--; r === 1 && a === 0",
        "let a, r; a = 1; r = --a; r === 0 && a === 0",
        // A postfix update yields the old value converted to a number.
        "let t = true; let r = t++; r === 1 && t === 2",
        "let f = false; let r = f--; r === 0 && f === -1",
        "let n = null; let r = n++; r === 0 && n === 1",
        // Parenthesized targets, parameters, and a line break before `++`.
        "let p = 1; (p)++; --((p)); ++(p); p === 2",
        "function g(x) { x--; return x; } g(3) === 2",
        "let a = 1, b = 1\na\n++b\nb === 2 && a === 1",
        // Postfix binds tighter than binary operators.
        "let a = 1, b = 1; let c = a+++b; c === 2 && a === 2 && b === 1",
        "let a = 3; a++ ** 2 === 9 && ++a ** 2 === 25",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
    assert!(num("let u; u++").is_nan());
    assert!(num("let u; ++u; u").is_nan());
    assert_eq!(num("let n = null; ++n"), 1.0);
    assert_eq!(num("let z = -0; z++; z"), 1.0);
    assert_eq!(num("let z = 0; --z"), -1.0);
    // The old value keeps its sign: `-0` is returned as `-0`.
    assert_eq!(num("let z = -0; z--").to_bits(), (-0.0f64).to_bits());
    assert_eq!(num("let x = 1.5; x--; x"), 0.5);
    assert_eq!(num("let big = 2 ** 53; big++; big"), 2f64.powi(53));
    // A loop counter.
    assert_eq!(
        num("let i = 0, s = 0; while (i < 5) { s = s + i++; } s * 10 + i"),
        105.0
    );
    // A closure-captured counter lives in a binding cell.
    assert_eq!(
        num(
            "function mk() { let c = 0; return function () { return ++c; }; } const f = mk(); f(); f()"
        ),
        2.0
    );
    assert_eq!(
        num("let c = 10; const dec = () => c--; dec(); dec() * 100 + c"),
        908.0
    );

    // `const`: the read and conversion happen, then the TypeError.
    let (kind, message, line, column) = failure_at("const c = 1;\nc++;");
    assert_eq!(kind, VmErrorKind::TypeError);
    assert!(message.contains("constant variable 'c'"), "{message}");
    assert_eq!((line, column), (2, 1));
    assert_eq!(failure("const c = 1; --c").kind(), VmErrorKind::TypeError);
    assert_eq!(
        num("const c = 1; let r = 0; try { c++; } catch (e) { r = 1; } r * 10 + c"),
        11.0
    );
    // A captured `const` through a closure.
    assert_eq!(
        failure("const k = 1; const g = () => k++; g()").kind(),
        VmErrorKind::TypeError
    );
    // Temporal dead zone, static and through a closure's cell.
    let (kind, _, line, column) = failure_at("{ x++; let x = 1; }");
    assert_eq!((kind, line, column), (VmErrorKind::ReferenceError, 1, 3));
    assert_eq!(
        failure("function g() { return ++q; } g(); let q = 1;").kind(),
        VmErrorKind::ReferenceError
    );
    // Unresolvable names.
    let (kind, message, line, column) = failure_at("nope++");
    assert_eq!(
        (kind, message.as_str(), line, column),
        (VmErrorKind::ReferenceError, "nope is not defined", 1, 1)
    );
    assert_eq!(failure("--nope").kind(), VmErrorKind::ReferenceError);
    // A conversion failure is at the operator token.
    let (kind, _, line, column) = failure_at("let f = Object.create(null);\n  f ++;");
    assert_eq!((kind, line, column), (VmErrorKind::TypeError, 2, 5));
    let (kind, _, line, column) = failure_at("let v = __rt.vector();\n--v;");
    assert_eq!((kind, line, column), (VmErrorKind::TypeError, 2, 1));

    // No new opcode: ToNumber, then add or sub, then the binding's store.
    let ops = opcodes(&exe("let a = 1; a++; --a"));
    for op in [Opcode::ToNumber, Opcode::Add, Opcode::Sub, Opcode::SetLocal] {
        assert!(ops.contains(&op), "{op:?} missing from {ops:?}");
    }
    // An update's value is a number: no second conversion.
    let ops = opcodes(&exe("let a = 1; -a++"));
    assert_eq!(
        ops.iter().filter(|op| **op == Opcode::ToNumber).count(),
        1,
        "{ops:?}"
    );
}

/// Prefix and postfix `++`/`--` on property references `o.p` and `o[k]`
/// (clj-quickjs-76). The first seven cases are adapted from the member
/// groups of `test_inc_dec` in `research/quickjs/tests/test_language.js`,
/// revision 7d1c13c5d8f8 (`a = {x:true}` becomes `let a = {x: true}`, and
/// `assert(a.x, 2)` / `assert(r === 1 && a.x === 2, true)` the script's
/// value); see `docs/architecture.md`, "QuickJS references and
/// provenance". The other cases are written for this project.
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
fn update_expressions_on_properties() {
    // `test_inc_dec`, member groups.
    for src in [
        "let a = {x: true}; a.x++; a.x === 2",
        "let a = {x: true}; a.x--; a.x === 0",
        "let a = [true]; a[0]++; a[0] === 2",
        "let a, r; a = {x: true}; r = a.x++; r === 1 && a.x === 2",
        "let a, r; a = {x: true}; r = a.x--; r === 1 && a.x === 0",
        "let a, r; a = [true]; r = a[0]++; r === 1 && a[0] === 2",
        "let a, r; a = [true]; r = a[0]--; r === 1 && a[0] === 0",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
    for src in [
        // The key and the base are evaluated once.
        "let i = 0; const a = [1, 1]; a[i = i + 1]++; i === 1 && a[1] === 2",
        "let a = [0, 0], i = 0; a[i++]++; i === 1 && a[0] === 1 && a[1] === 0",
        "let n = 0; const o = {x: 1}; function get() { n = n + 1; return o; } \
         get().x++; n === 1 && o.x === 2",
        "let n = 0; const o = {x: 1}; const get = () => { n++; return o; }; \
         let r = --get()['x']; n === 1 && r === 0 && o.x === 0",
        // Prefix updates give the new number.
        "let o = {p: true}; let r = ++o.p; r === 2 && o.p === 2",
        "let o = {p: 1}; let r = --o['p']; r === 0 && o.p === 0",
        // Parenthesized targets, chains and nested references.
        "let o = {p: 1}; (o.p)++; ++(o['p']); --((o).p); o.p === 2",
        "let o = {a: {b: [1, 2]}}; o.a.b[1]++; ++o['a'].b[0]; o.a.b[0] === 2 && o.a.b[1] === 3",
        "let a = [[5]]; a[0][a[0][0]++ - 5]--; a[0][0] === 5",
        // A string key that is an array index, and a numeric name.
        "let a = [0]; a['0']++; a[0] === 1",
        "let o = {}; o[1.5] = 1; o[1.5]++; o['1.5'] === 2",
        // Writing past an array's end extends it (the value is NaN).
        "let a = []; a[2]++; a.length === 3",
        // A loop counter in an object.
        "let o = {n: 0}; for (let i = 0; i < 5; i++) o.n++; o.n === 5",
        // Postfix binds tighter than binary operators.
        "let o = {a: 1, b: 1}; let c = o.a+++o.b; c === 2 && o.a === 2 && o.b === 1",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
    // An absent property reads as undefined: NaN is yielded and written.
    assert!(num("let o = {}; o.x++").is_nan());
    assert!(num("let o = {}; o.x++; o.x").is_nan());
    assert!(num("let o = {}; ++o['y']").is_nan());
    // The old value keeps its sign: `-0` is returned as `-0`.
    assert_eq!(num("let o = {z: -0}; o.z--").to_bits(), (-0.0f64).to_bits());
    assert_eq!(num("let o = {n: null}; o.n++; o.n"), 1.0);

    // A `null`/`undefined` base is a catchable TypeError, reported at the
    // member's `.` or `[`, before the key is converted or anything written.
    let (kind, message, line, column) = failure_at("let o = null;\no.x++;");
    assert_eq!(
        (kind, line, column),
        (VmErrorKind::TypeError, 2, 2),
        "{message}"
    );
    let (kind, _, line, column) = failure_at("null.x++");
    assert_eq!((kind, line, column), (VmErrorKind::TypeError, 1, 5));
    let (kind, _, line, column) = failure_at("let u;\n--u[0];");
    assert_eq!((kind, line, column), (VmErrorKind::TypeError, 2, 4));
    assert_eq!(
        failure("let u; let k = {}; u[k]++").kind(),
        VmErrorKind::TypeError
    );
    assert_eq!(
        string("let r = 'none'; try { null.x++; } catch (e) { r = 'caught'; } r"),
        "caught"
    );
    assert_eq!(
        string("let r = 'none', u; try { --u['k']; } catch (e) { r = 'caught'; } r"),
        "caught"
    );
    // A failed write (a non-writable property, clj-quickjs-119) is a
    // catchable TypeError at the `.` or `[`, after the read and the
    // conversion; the property keeps its value.
    let frozen = "let o = {}; Object.defineProperty(o, 'k', {value: 1});";
    let (kind, message, line, column) = failure_at(&format!("{frozen}\no.k++;"));
    assert_eq!(
        (kind, line, column),
        (VmErrorKind::TypeError, 2, 2),
        "{message}"
    );
    let (kind, _, line, column) = failure_at(&format!("{frozen}\n--o['k'];"));
    assert_eq!((kind, line, column), (VmErrorKind::TypeError, 2, 4));
    assert_eq!(
        num(&format!(
            "{frozen} let r = 0; try {{ o.k++; }} catch (e) {{ r = 1; }} r * 10 + o.k"
        )),
        11.0
    );
    // The default object string becomes NaN in a numeric update.
    assert!(num("let o = {p: {}}; o.p++; o.p").is_nan());
    let (kind, _, line, column) = failure_at("let o = {p: Object.create(null)};\no.p ++;");
    assert_eq!((kind, line, column), (VmErrorKind::TypeError, 2, 5));
    // Intrinsics of the reserved binding stay compile errors.
    for src in ["__rt.count++;", "--__rt.count;", "__rt.count.x++;"] {
        let e = compile_script("t", src).unwrap_err();
        assert_eq!(e.kind, SyntaxErrorKind::ReservedWord, "{src}: {e}");
    }

    // `o[k]` converts its key once, with `to_property_key`, and reads with
    // the reference duplicated; `o.p` duplicates its base.
    let ops = opcodes(&exe("let o = {}, k = 'p'; o[k]++;"));
    for op in [
        Opcode::ToPropertyKey,
        Opcode::Dup2,
        Opcode::GetElem,
        Opcode::ToNumber,
        Opcode::PutElem,
    ] {
        assert!(ops.contains(&op), "{op:?} missing from {ops:?}");
    }
    let ops = opcodes(&exe("let o = {}; ++o.p;"));
    for op in [Opcode::Dup, Opcode::GetField, Opcode::Add, Opcode::PutField] {
        assert!(ops.contains(&op), "{op:?} missing from {ops:?}");
    }
    // An update's value is a number: no second conversion.
    let ops = opcodes(&exe("let o = {p: 1}; -o.p++"));
    assert_eq!(
        ops.iter().filter(|op| **op == Opcode::ToNumber).count(),
        1,
        "{ops:?}"
    );
}

/// Compound assignment on property references `o.p op= v` and `o[k] op= v`
/// (clj-quickjs-260): the base and the key once, the key converted once,
/// then the read, the right operand, the operator and the write. Synthetic
/// fixtures, not light-cljs output.
#[test]
fn compound_assignment_on_properties() {
    for src in [
        "let o = {x: 1}; o.x += 2; o.x === 3",
        "let o = {x: 3}; o['x'] *= 2; o.x === 6",
        // The key is evaluated once.
        "let a = [1, 1]; let i = 0; a[i = i + 1] += 4; i === 1 && a[1] === 5 && a[0] === 1",
        "let a = [0, 0], i = 0; a[i++] -= 1; i === 1 && a[0] === -1 && a[1] === 0",
        // The base is evaluated once.
        "let n = 0; const o = {x: 1}; function get() { n = n + 1; return o; } \
         get().x += 1; n === 1 && o.x === 2",
        "let n = 0; const o = {x: 1}; const get = () => { n++; return o; }; \
         get()['x'] **= 3; get()['x'] += get().x; n === 3 && o.x === 2",
        // The expression's value is the written value; right-associative.
        "let o = {x: 1}; let r = (o.x += 2); r === 3 && o.x === 3",
        "let o = {a: 10, b: 2}; o.a -= o.b *= 3; o.a === 4 && o.b === 6",
        "let o = {a: 1}, b = 2; o.a += b += 3; o.a === 6 && b === 5",
        "let a = [1]; let b = 2; b *= a[0] += 4; b === 10 && a[0] === 5",
        // Parenthesized targets, chains and nested references.
        "let o = {p: 2}; (o.p) *= 5; ((o)['p']) -= 1; o.p === 9",
        "let o = {a: {b: [1, 2]}}; o.a.b[1] += 10; o['a'].b[0] -= 1; \
         o.a.b[0] === 0 && o.a.b[1] === 12",
        "let a = [[5]]; a[0][a[0][0] -= 5] += 1; a[0][0] === 1",
        // Every operator.
        "let o = {v: 7}; o.v -= 1; o.v /= 4; o.v %= 1; o.v === 0.5",
        "let o = {v: 2}; o.v **= 10; o.v === 1024",
        "let o = {v: 12}; o.v &= 10; o.v |= 1; o.v ^= 3; o.v === 10",
        "let o = {v: 1}; o.v <<= 4; o.v >>= 1; o.v === 8",
        "let o = {v: -16}; o.v >>>= 28; o['v'] === 15",
        "let o = {v: -16}; o.v >>= 2; o.v === -4",
        // `+=` concatenates when either value is a string; the others
        // apply ToNumber.
        "let o = {s: 'a'}; o.s += 1; o.s += true; o.s === 'a1true'",
        "let o = {n: 1}; o.n += '2'; o.n === '12'",
        "let o = {s: '6'}; o.s *= '2'; o.s === 12",
        "let o = {t: true, u: null}; o.t += 1; o.u -= 1; o.t === 2 && o.u === -1",
        // A string key that is an array index, and a numeric name.
        "let a = [1]; a['0'] += 1; a[0] === 2",
        "let o = {}; o[1.5] = 1; o[1.5] += 1; o['1.5'] === 2",
        // A loop accumulator in an object and an array.
        "let o = {n: 0}, a = [0]; for (let i = 1; i <= 4; i++) { o.n += i; a[0] -= i; } \
         o.n === 10 && a[0] === -10",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
    // An absent property reads as undefined: NaN for the numeric operators,
    // concatenation for `+=` with a string.
    assert!(num("let o = {}; o.x += 1; o.x").is_nan());
    assert_eq!(string("let o = {}; o.k += 'v'; o.k"), "undefinedv");
    // Writing past an array's end extends it.
    assert_eq!(num("let a = []; a[2] |= 4; a.length * 10 + a[2]"), 34.0);

    // The order: base, key, read, right operand, write. The right operand
    // sees the old value (it was not yet written) and replaces it; the
    // result uses the value read before it ran, and the write comes after
    // it.
    assert_eq!(
        string(
            "let log = ''; const o = {x: 1}; \
             function base() { log += 'b'; return o; } \
             function key() { log += 'k'; return 'x'; } \
             function rhs() { log += 'r' + o.x; o.x = 100; return 2; } \
             let r = base()[key()] += rhs(); \
             log += ',' + r + ',' + o.x; log"
        ),
        "bkr1,3,3"
    );
    assert_eq!(
        string(
            "let log = ''; const o = {x: 1}; \
             const base = () => { log += 'b'; return o; }; \
             base().x *= (log += 'r' + o.x, o.x = 7, 5); \
             log += ',' + o.x; log"
        ),
        "br1,5"
    );

    // A `null`/`undefined` base is a catchable TypeError at the `.` or
    // `[`, after the key but before the right operand runs.
    let (kind, message, line, column) = failure_at("let o = null;\no.x += 1;");
    assert_eq!(
        (kind, line, column),
        (VmErrorKind::TypeError, 2, 2),
        "{message}"
    );
    let (kind, _, line, column) = failure_at("null.x += 1");
    assert_eq!((kind, line, column), (VmErrorKind::TypeError, 1, 5));
    let (kind, _, line, column) = failure_at("let u;\nu[0] -= 1;");
    assert_eq!((kind, line, column), (VmErrorKind::TypeError, 2, 2));
    assert_eq!(
        string("let r = 'none'; try { null.x += 1; } catch (e) { r = 'caught'; } r"),
        "caught"
    );
    assert_eq!(
        string(
            "let log = '', u; \
             try { u[(log += 'k', 'p')] += (log += 'r', 1); } catch (e) { log += 'c'; } log"
        ),
        "kc"
    );
    assert_eq!(
        string(
            "let log = ''; \
             try { null.x *= (log += 'r', 1); } catch (e) { log += 'c'; } log"
        ),
        "c"
    );

    // A failed write (a non-writable property, clj-quickjs-119): in strict
    // code a catchable TypeError at the `.` or `[` after the right operand;
    // the property keeps its value.
    let frozen = "let o = {}; Object.defineProperty(o, 'k', {value: 1});";
    let (kind, message, line, column) = failure_at(&format!("{frozen}\no.k += 1;"));
    assert_eq!(
        (kind, line, column),
        (VmErrorKind::TypeError, 2, 2),
        "{message}"
    );
    let (kind, _, line, column) = failure_at(&format!("{frozen}\no['k'] *= 2;"));
    assert_eq!((kind, line, column), (VmErrorKind::TypeError, 2, 2));
    assert_eq!(
        string(&format!(
            "{frozen} let log = ''; \
             try {{ o.k += (log += 'r', 1); }} catch (e) {{ log += 'c'; }} log + o.k"
        )),
        "rc1"
    );
    // In sloppy code the write does nothing; the value is still the
    // operator's result. A strict function in the same script throws.
    assert_eq!(
        sloppy_eval(&format!(
            "{frozen} let r = (o.k += 5); let s = (o['k'] -= 1); r === 6 && s === 0 && o.k === 1"
        )),
        ValueView::Bool(true)
    );
    assert_eq!(
        sloppy_eval(&format!(
            "{frozen} function f() {{ 'use strict'; o.k += 3; }} \
             let r = 'none'; try {{ f(); }} catch (e) {{ r = 'caught'; }} r === 'caught' && o.k === 1"
        )),
        ValueView::Bool(true)
    );

    assert!(num("let o = {p: {}}; o.p *= 2; o.p").is_nan());
    assert_eq!(
        string("let o = {p: 1}; o['p'] += {}; o.p"),
        "1[object Object]"
    );
    // Failed conversions still name the `op=` token.
    let (kind, _, line, column) = failure_at("let o = {p: Object.create(null)};\no.p *= 2;");
    assert_eq!((kind, line, column), (VmErrorKind::TypeError, 2, 5));
    let (kind, _, line, column) = failure_at("let o = {p: 1};\no['p'] += Object.create(null);");
    assert_eq!((kind, line, column), (VmErrorKind::TypeError, 2, 8));
    // Intrinsics of the reserved binding stay compile errors.
    for src in ["__rt.count += 1;", "__rt.count -= 1;", "__rt[0] += 1;"] {
        let e = compile_script("t", src).unwrap_err();
        assert_eq!(e.kind, SyntaxErrorKind::ReservedWord, "{src}: {e}");
    }
    // Logical assignment stays unsupported, on properties too.
    for src in ["o.x &&= 1;", "o[0] ||= 1;", "o.x ??= 1;"] {
        let e = compile_script("t", src).unwrap_err();
        assert_eq!(e.kind, SyntaxErrorKind::Unsupported, "{src}: {e}");
    }

    // `o[k]` converts its key once and reads with the reference
    // duplicated; `o.p` duplicates its base.
    let ops = opcodes(&exe("let o = {}, k = 'p'; o[k] += 1;"));
    for op in [
        Opcode::ToPropertyKey,
        Opcode::Dup2,
        Opcode::GetElem,
        Opcode::Plus,
        Opcode::PutElem,
    ] {
        assert!(ops.contains(&op), "{op:?} missing from {ops:?}");
    }
    assert_eq!(
        ops.iter()
            .filter(|op| **op == Opcode::ToPropertyKey)
            .count(),
        1,
        "{ops:?}"
    );
    let ops = opcodes(&exe("let o = {}; o.p -= 1;"));
    for op in [Opcode::Dup, Opcode::GetField, Opcode::Sub, Opcode::PutField] {
        assert!(ops.contains(&op), "{op:?} missing from {ops:?}");
    }
    // A numeric compound assignment's value is a number: no conversion
    // before `+`.
    let ops = opcodes(&exe("let o = {p: 1}; 1 + (o.p *= 2)"));
    assert!(ops.contains(&Opcode::Add), "{ops:?}");
    assert!(!ops.contains(&Opcode::Plus), "{ops:?}");
}

/// Compound assignment on identifier bindings (clj-quickjs-84): read, then
/// the right operand, then the binary operator's own instructions, then the
/// store.
#[test]
fn compound_assignment_on_bindings() {
    assert_eq!(
        num("let a = 7; a += 3; a -= 1; a *= 2; a /= 3; a %= 4; a"),
        2.0
    );
    for src in [
        // The expression's value is the stored value.
        "let b = 1; let r = (b += 2); r === 3 && b === 3",
        // Right-associative.
        "let a = 10, b = 2; a -= b *= 3; a === 4 && b === 6",
        "let a = 1, b = 2; a += b += 3; a === 6 && b === 5",
        // Parenthesized targets and parameters.
        "let p = 2; (p) *= 5; ((p)) -= 1; p === 9",
        "function g(x) { x %= 3; return x; } g(7) === 1",
        // ToNumber on `undefined`, `null`, booleans and (except for `+=`)
        // strings (clj-quickjs-67).
        "let t = true; t += 1; t === 2",
        "let n = null; n -= 1; n === -1",
        "let f = false; f *= 5; f === 0",
        r#"let s = "8"; s -= "2"; s /= 2; s === 3"#,
        r#"let s = " 0x10 "; s |= 1; s === 17"#,
        // The bitwise, shift and `**` forms (clj-quickjs-62, -63).
        "let a = 3; a **= 2; a === 9",
        "let a = 6; a &= 3; a === 2",
        "let a = 6; a |= 1; a === 7",
        "let a = 6; a ^= 3; a === 5",
        "let a = 1; a <<= 4; a === 16",
        "let a = -16; a >>= 2; a === -4",
        "let a = -1; a >>>= 28; a === 15",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
    // The old value is read before the right operand runs.
    assert_eq!(
        num("let a = 1; function f() { a = 10; return 5; } a += f(); a"),
        6.0
    );
    assert_eq!(num("let a = 2; a *= (a = 5) + 1; a"), 12.0);
    // `+=` may yield a string, so an enclosing operator converts it; the
    // other compound forms yield numbers.
    assert_eq!(string(r#"let s = ""; (s += "a") + 1"#), "a1");
    assert_eq!(num(r#"let s = ""; 2 * (s += "3")"#), 6.0);
    assert_eq!(num(r#"let s = "4"; (s -= 1) + 1"#), 4.0);
    assert!(num("let u; u += 1").is_nan());
    assert!(num("let u; u -= 1; u").is_nan());
    // `+=` concatenates when either value is a string (clj-quickjs-66).
    assert_eq!(string(r#"let s = ""; s += "a"; s += 1; s"#), "a1");
    assert_eq!(string(r#"let n = 1; n += "b"; n"#), "1b");
    // A closure-captured counter lives in a binding cell.
    assert_eq!(
        num(
            "function mk() { let c = 0; return (d) => c += d; } const f = mk(); f(2); f(3) * 10 + f(0)"
        ),
        55.0
    );
    assert_eq!(
        num("let c = 1; const g = () => c; c += 4; c *= g(); c"),
        25.0
    );

    // `const`: the read and the right operand run, then the TypeError.
    let (kind, message, line, column) = failure_at("const c = 1;\nc += 1;");
    assert_eq!(kind, VmErrorKind::TypeError);
    assert!(message.contains("constant variable 'c'"), "{message}");
    assert_eq!((line, column), (2, 1));
    assert_eq!(
        num("const c = 1; let r = 0; try { c -= (r = 5); } catch (e) { r = r + 1; } r * 10 + c"),
        61.0
    );
    assert_eq!(
        failure("const k = 1; const g = () => k **= 2; g()").kind(),
        VmErrorKind::TypeError
    );
    // Temporal dead zone and unresolvable names fail at the read, before
    // the right operand is evaluated.
    let (kind, _, line, column) = failure_at("{ x += 1; let x = 1; }");
    assert_eq!((kind, line, column), (VmErrorKind::ReferenceError, 1, 3));
    assert_eq!(
        failure("function g() { return q |= 1; } g(); let q = 1;").kind(),
        VmErrorKind::ReferenceError
    );
    let (kind, message, line, column) = failure_at("nope += 1");
    assert_eq!(
        (kind, message.as_str(), line, column),
        (VmErrorKind::ReferenceError, "nope is not defined", 1, 1)
    );
    assert_eq!(
        num(
            "let r = 0; try { nope += (r = 1); } catch (e) {} try { { y -= (r = 2); let y; } } catch (e) {} r"
        ),
        0.0
    );
    // The operator (and its conversions) runs after the right operand, so
    // the right operand's failure comes first.
    assert_eq!(
        failure("let a = () => 1; a -= nope").kind(),
        VmErrorKind::ReferenceError
    );
    assert_eq!(
        failure("let a = __rt.vector(); a <<= (() => { throw 1; })()").kind(),
        VmErrorKind::Thrown
    );
    // A failing operator is reported at the `op=` token.
    let (kind, _, line, column) = failure_at("let f = Object.create(null);\n  f -= 1;");
    assert_eq!((kind, line, column), (VmErrorKind::TypeError, 2, 5));
    let (kind, _, line, column) = failure_at("let v = __rt.vector();\nv += 1;");
    assert_eq!((kind, line, column), (VmErrorKind::TypeError, 2, 3));
    // The reserved intrinsic binding cannot be assigned.
    let e = compile_script("t", "__rt += 1;").unwrap_err();
    assert_eq!(e.kind, SyntaxErrorKind::ReservedWord, "{e}");

    // No new opcode: `+=` on a binding is `plus` (it may concatenate), the
    // others convert both values and use the numeric instruction.
    let ops = opcodes(&exe("let a = 1; a += 1; a -= 2; a <<= 1"));
    for op in [
        Opcode::Plus,
        Opcode::ToNumberPair,
        Opcode::Sub,
        Opcode::Shl,
        Opcode::SetLocal,
    ] {
        assert!(ops.contains(&op), "{op:?} missing from {ops:?}");
    }

    // A task yields between the read and the store and resumes there.
    let looping = exe("let a = 0, i = 0; while (i < 50) { a += i * 2; a -= i; i += 1; } a");
    let task = Runtime::new().spawn(&looping, &[]).unwrap();
    let mut yields = 0;
    let result = loop {
        match task
            .run_with(Budget::instructions(2), &mut NoEffects)
            .unwrap()
        {
            RunOutcome::Yielded => yields += 1,
            RunOutcome::Complete(v) => break v.view(),
            other => panic!("unexpected {other:?}"),
        }
    };
    assert_eq!(result, ValueView::Number(1225.0));
    assert!(yields >= 100, "{yields} yields");
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

/// Object literals with literal keys, `o.name` reads and `o.name = v`
/// assignments (clj-quickjs-114). The `{x: 1, if: 2, async: 3}` and nested
/// `{a: {b: 2}}` rows follow `test_op2` (line 195) and
/// `test_optional_chaining` (line 610) of
/// `research/quickjs/tests/test_language.js`, revision 7d1c13c5d8f8
/// (restated as Rust assertions on the expression's value); see
/// `docs/architecture.md`, "QuickJS references and provenance". The rest
/// are synthetic fixtures, not light-cljs output.
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
fn object_literals_and_named_properties() {
    let o = "let o = {x: 1, if: 2, async: 3};";
    assert_eq!(num(&format!("{o} o.x + o.if + o.async")), 6.0);
    assert_eq!(eval(&format!("{o} o.missing")), ValueView::Undefined);
    assert_eq!(num(&format!("{o} o.y = 4")), 4.0);
    assert_eq!(num(&format!("{o} o.y = 4; o.y")), 4.0);
    assert_eq!(num(&format!("{o} o.x = 5; o.x + o.if")), 7.0);
    assert_eq!(num("({x: 1, x: 2}).x"), 2.0);
    assert_eq!(num("({a: {b: 2}}).a.b"), 2.0);
    assert_eq!(num("let get = 1; ({get}).get"), 1.0);
    assert_eq!(
        num("let a = 1, b = 2; let p = {a, b, c: a + b}; p.a + p.b * p.c"),
        7.0
    );
    assert_eq!(eval("({}) !== ({})"), ValueView::Bool(true));
    assert_eq!(eval(&format!("{o} o === o")), ValueView::Bool(true));
    assert_eq!(string("typeof {}"), "object");
    // String and numeric keys: a numeric key is its ToString.
    assert_eq!(num(r#"({"b": 2, 3: 4, 1.50: 5, 0x10: 6}).b"#), 2.0);
    assert_eq!(num(r#"let k = {"if": 7}; k.if"#), 7.0);
    assert_eq!(num(r"({a: 1, \u0069f: 2}).if + ({n\u{65}w: 3}).new"), 5.0);
    // Keys of every kind, reserved words included, and a trailing comma.
    assert_eq!(
        num(
            "let r = {get: 2, set: 3, async: 4, static: 5, new: 6, typeof: 7,}; \
             r.get + r.set + r.async + r.static + r.new + r.typeof"
        ),
        27.0
    );
    // Assignment is right-associative and leaves the assigned value.
    assert_eq!(num("let p = {}, q = {}; p.v = q.v = 3; p.v + q.v"), 6.0);
    // A property may hold any value, functions and the object itself
    // included.
    assert_eq!(
        num("let m = {f: function (x) { return x * 2; }}; let f = m.f; f(21)"),
        42.0
    );
    assert_eq!(
        num("let c = {}; c.self = c; c.self.self.self === c ? 1 : 0"),
        1.0
    );

    // Reading or writing a property of undefined or null throws a TypeError
    // that `catch` receives, located at the `.` (clj-quickjs-52).
    assert_eq!(
        string("let u; let r = 'none'; try { u.x; } catch (e) { r = 'caught'; } r"),
        "caught"
    );
    assert_eq!(
        string("let r = 'none'; try { null.x = 1; } catch (e) { r = 'caught'; } r"),
        "caught"
    );
    assert_eq!(
        failure_at("let u;\nu.x;"),
        (
            VmErrorKind::TypeError,
            "cannot read property 'x' of undefined".to_string(),
            2,
            2
        )
    );
    assert_eq!(
        failure_at("let n = null;\n  n.x = 1;"),
        (
            VmErrorKind::TypeError,
            "cannot set property 'x' of null".to_string(),
            2,
            4
        )
    );
    // The right-hand side runs before the base is checked (PutValue).
    assert_eq!(
        num("let hit = 0; try { null.x = (hit = 1); } catch (e) {} hit"),
        1.0
    );

    // Evaluation order, observed through a closure-captured counter: the
    // values of a literal left to right, and the base of an assignment
    // before its right-hand side.
    let log = "let log = 0; function f(n) { log = log * 10 + n; return n; }";
    assert_eq!(num(&format!("{log} ({{a: f(1), b: f(2)}}); log")), 12.0);
    assert_eq!(
        num(&format!(
            "{log} let t = {{}}; function g() {{ f(1); return t; }} \
             function h() {{ f(2); return 9; }} g().x = h(); log * 100 + t.x"
        )),
        1209.0
    );
    // Duplicate keys: every value is evaluated, the last one wins.
    assert_eq!(
        num(&format!(
            "{log} let d = {{x: f(1), x: f(2)}}; log * 10 + d.x"
        )),
        122.0
    );
}

/// A call whose callee is a property reference passes its base as `this`
/// (clj-quickjs-116); every other call passes `undefined` (strict code).
#[test]
fn method_calls_bind_this() {
    let o = "let o = {x: 1, m: function () { return this.x; }};";
    assert_eq!(num(&format!("{o} o.m()")), 1.0);
    // A parenthesized member callee keeps its receiver.
    assert_eq!(num(&format!("{o} (o.m)()")), 1.0);
    assert_eq!(num(&format!("{o} ((o.m))()")), 1.0);
    // A computed member callee is a method call too (clj-quickjs-118).
    assert_eq!(num(&format!("{o} o[\"m\"]()")), 1.0);
    // A comma or assignment result, or a plain reference, is not a member
    // callee: `this` is undefined, so `this.x` throws a catchable TypeError.
    for call in ["(0, o.m)()", "let f = o.m; f()", "let f; (f = o.m)()"] {
        assert_eq!(
            string(&format!(
                "{o} let r = 'none'; try {{ {call}; }} catch (e) {{ r = 'caught'; }} r"
            )),
            "caught",
            "{call}"
        );
        let (kind, message, ..) = failure_at(&format!("{o}\n{call};"));
        assert_eq!(kind, VmErrorKind::TypeError, "{call}");
        assert_eq!(message, "cannot read property 'x' of undefined", "{call}");
    }
    assert_eq!(
        eval("function g() { return this; } g()"),
        ValueView::Undefined
    );
    assert_eq!(num(&format!("{o} ({{inner: o}}).inner.m()")), 1.0);
    // The base is evaluated once.
    assert_eq!(
        num(&format!(
            "{o} let n = 0; function getO() {{ n = n + 1; return o; }} getO().m() * 10 + n"
        )),
        11.0
    );
    // `this` is the receiver itself, not a copy.
    assert_eq!(
        eval("let p = {me: function () { return this; }}; p.me() === p"),
        ValueView::Bool(true)
    );
    // Writes through `this` (as QuickJS tests/test_closure.js `obj.set(10)`).
    assert_eq!(
        num("let p = {v: 0, set: function (v) { this.v = v; }}; p.set(10); p.v"),
        10.0
    );
    // Methods calling methods on `this`; a nested plain call sees
    // `undefined` and the caller's `this` is intact after it returns.
    assert_eq!(
        num("let c = {v: 2, get: function () { return this.v; }, \
             twice: function () { return this.get() * 2; }}; c.twice()"),
        4.0
    );
    assert_eq!(
        num(
            "let c = {v: 3, m: function () { function inner() { return this; } \
             return inner() === undefined ? this.v : -1; }}; c.m()"
        ),
        3.0
    );
    // A default parameter sees the function's `this`.
    assert_eq!(
        num("let c = {v: 5, m: function (a = this) { return a.v; }}; c.m()"),
        5.0
    );
    // The same function as a method of two objects.
    assert_eq!(
        num(
            "function get() { return this.v; } let a = {v: 1, get}; let b = {v: 2, get}; \
             a.get() * 10 + b.get()"
        ),
        12.0
    );
    // Through `throw`/`catch` and `finally` in the method.
    assert_eq!(
        num(
            "let c = {v: 7, m: function () { try { throw 1; } catch (e) { return this.v; } \
             finally { this.v = 8; } }}; c.m() * 10 + c.v"
        ),
        78.0
    );
    // A task's entry function and a native's callback are plain calls.
    let entry = compile_function("function f() { return this; }").unwrap();
    match run(&entry, &[]) {
        RunOutcome::Complete(v) => assert_eq!(v.view(), ValueView::Undefined),
        other => panic!("{other:?}"),
    }
    assert_eq!(
        eval("__rt.reduce(function (acc, x) { return this; }, 0, __rt.vector(1))"),
        ValueView::Undefined
    );
    // `this` compiles to `get_this`.
    let script = exe(&format!("{o} o.m()"));
    let ops = opcodes(&script.function(0).unwrap());
    assert!(ops.contains(&Opcode::GetThis), "{ops:?}");

    // Sloppy code: an object receiver is `this` as is; an `undefined` one
    // would be replaced by the global object (clj-quickjs-104), so it fails
    // explicitly instead of giving a wrong value.
    let sloppy = |src: &str| {
        let exe = compile_script_with("sloppy", src, ScriptOptions::SLOPPY)
            .unwrap_or_else(|e| panic!("{e}"));
        run(&exe, &[])
    };
    match sloppy("let p = {me: function () { return this; }}; p.me() === p") {
        RunOutcome::Complete(v) => assert_eq!(v.view(), ValueView::Bool(true)),
        other => panic!("{other:?}"),
    }
    match sloppy("function g() { return this; } g()") {
        RunOutcome::Failed(e) => {
            assert_eq!(e.kind(), VmErrorKind::UnsupportedOperand, "{e}");
            assert!(e.message().contains("sloppy-mode"), "{e}");
        }
        other => panic!("{other:?}"),
    }
    // A strict function in a sloppy script sees `undefined`.
    match sloppy("function g() { 'use strict'; return this; } g()") {
        RunOutcome::Complete(v) => assert_eq!(v.view(), ValueView::Undefined),
        other => panic!("{other:?}"),
    }
}

/// `new` on ordinary function constructors (clj-quickjs-71; the constructor
/// `F` and the `new F(2)` case of `test_op2` in
/// `research/quickjs/tests/test_language.js`, revision 7d1c13c5d8f8; QuickJS
/// copyright and MIT license notice: see
/// `loose_equality_with_a_boolean_left_operand`).
#[test]
fn new_operator_constructs_objects() {
    let f = "function F(x) { this.x = x; }";
    assert_eq!(num(&format!("{f} const b = new F(2); b.x")), 2.0);
    assert_eq!(eval(&format!("{f} (new F).x")), ValueView::Undefined);
    assert_eq!(
        eval(&format!("{f} new F(1) !== new F(1)")),
        ValueView::Bool(true)
    );
    // The new object inherits from `F.prototype`, whose `constructor` is
    // `F`; instances share it and see properties added to it later.
    assert_eq!(
        eval(&format!(
            "{f} const a = new F(1), b = new F(2); F.prototype.k = 7; \
             a.constructor === F && b.k === 7 && a.k === 7 && a.x === 1"
        )),
        ValueView::Bool(true)
    );
    assert_eq!(
        num(
            "function P() {} P.prototype = {m: function () { return this.v * 2; }}; \
             function Q(v) { this.v = v; } Q.prototype = P.prototype; new Q(21).m()"
        ),
        42.0
    );
    // A `prototype` that is not an object: the new object inherits from
    // `Object.prototype` instead.
    assert_eq!(
        eval(
            "function N() {} N.prototype = 5; const n = new N(); \
             n.constructor === Object && n.hasOwnProperty === Object.prototype.hasOwnProperty"
        ),
        ValueView::Bool(true)
    );
    // Return override: an object result replaces the new object, any other
    // result is ignored.
    assert_eq!(
        num("function G() { this.y = 1; return {y: 2}; } new G().y"),
        2.0
    );
    assert_eq!(num("function H() { this.z = 1; return 5; } new H().z"), 1.0);
    assert_eq!(num("function U() { this.z = 3; return; } new U().z"), 3.0);
    assert_eq!(
        num("function U() { this.z = 3; return null; } new U().z"),
        3.0
    );
    assert_eq!(
        eval(
            "const fn = function () { return 1; }; \
             function R() { this.r = 1; return fn; } new R() === fn"
        ),
        ValueView::Bool(true)
    );
    // Through try/finally: the return a finally block completes is checked.
    assert_eq!(
        num(
            "function T() { try { this.a = 1; return 2; } finally { this.b = 3; } } \
             const t = new T(); t.a * 10 + t.b"
        ),
        13.0
    );
    // Arguments, nesting, and the result as a callee or a base.
    assert_eq!(
        num("function S(a, b) { this.s = a + b; } new S(1, 2).s + new S(new S(3, 4).s, 1).s"),
        11.0
    );
    assert_eq!(
        num("function K() { return function () { return 9; }; } new K()()"),
        9.0
    );
    assert_eq!(
        num("const ns = {C: function (v) { this.v = v; }}; new ns.C(6).v"),
        6.0
    );
    assert_eq!(
        num(
            "function A() { this.n = 1; } function B() { return new A(); } new new B().constructor().n"
        ),
        1.0
    );
    // A plain call of a function is not a construction (strict: `this` is
    // undefined), also from inside a constructor, whose `this` is intact
    // afterwards.
    assert_eq!(
        eval("function P() { return this; } P()"),
        ValueView::Undefined
    );
    assert_eq!(
        num("function inner() { return this === undefined ? 1 : 0; } \
             function O() { this.u = inner(); this.w = this.u + 1; } new O().w"),
        2.0
    );
    // A sloppy-mode constructor sees the new object as well.
    let sloppy = compile_script_with(
        "test",
        "function W() { this.w = 4; } new W().w",
        ScriptOptions::SLOPPY,
    )
    .unwrap();
    match run(&sloppy, &[]) {
        RunOutcome::Complete(v) => assert_eq!(v.view(), ValueView::Number(4.0)),
        other => panic!("{other:?}"),
    }
    // Not constructors: catchable TypeErrors, reported at the `new` token.
    for (src, message) in [
        ("new 1", "1 is not a constructor"),
        (
            "const v = __rt.vector(); new v",
            "vector is not a constructor",
        ),
        ("new undefined", "undefined is not a constructor"),
        (
            "const a = () => 1; new a()",
            "function is not a constructor",
        ),
        ("new Object()", "function is not a constructor"),
        ("const o = {}; new o()", "object is not a constructor"),
    ] {
        assert_eq!(
            string(&format!(
                "let r = 'none'; try {{ {src}; }} catch (e) {{ r = 'caught'; }} r"
            )),
            "caught",
            "{src}"
        );
        let (kind, got, line, column) = failure_at(&format!("\n{src};"));
        assert_eq!(kind, VmErrorKind::TypeError, "{src}");
        assert_eq!(got, message, "{src}");
        let at = src.rfind("new").unwrap() as u32 + 1;
        assert_eq!((line, column), (2, at), "{src}");
    }
    // The callee and the arguments are evaluated before the check.
    assert_eq!(
        string(
            "let log = ''; function c() { log = log + 'c'; return 1; } \
             function a() { log = log + 'a'; return 0; } \
             try { new (c())(a()); } catch (e) { log = log + 'e'; } log"
        ),
        "cae"
    );
    // `new` compiles to `construct`.
    assert!(opcodes(&exe(&format!("{f} new F(1)"))).contains(&Opcode::Construct));
    // The `docs/grammar.md` example lines, each true.
    for src in [
        "function F(x) { this.x = x; } const b = new F(2); b.x === 2 && (new F).x === undefined && new F(1) !== new F(1);",
        "function G() { this.y = 1; return {y: 2}; } function H() { this.z = 1; return 5; } new G().y === 2 && new H().z === 1;",
        "function P(v) { this.v = v; } P.prototype.twice = function () { return this.v * 2; }; new P(3).twice() === 6 && new P(3).constructor === P;",
        "let r = false; try { new Object(); } catch (e) { r = true; } r;",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
}

/// `v instanceof f` (clj-quickjs-75): InstanceofOperator and
/// OrdinaryHasInstance, compiled to `instanceof`. The `({}) instanceof
/// String` case of the pellet waits for the `String` constructor
/// (clj-quickjs-72) and `e instanceof ReferenceError` for the Error
/// constructors; `[].constructor` (the realm's `Array`) stands in for
/// another built-in constructor. The `a instanceof Object` case is adapted
/// from `test_op2` in `research/quickjs/tests/test_language.js`, revision
/// 7d1c13c5d8f8 (QuickJS copyright and MIT license notice: see
/// `loose_equality_with_a_boolean_left_operand`).
#[test]
fn instanceof_operator() {
    let f = "function F() {} function G() {} G.prototype = new F();";
    for src in [
        "new F() instanceof F",
        "!(({}) instanceof F)",
        "!(1 instanceof F)",
        "!('s' instanceof F) && !(undefined instanceof F) && !(null instanceof F)",
        "({}) instanceof Object",
        "new F() instanceof Object",
        "!(new F() instanceof G)",
        // A two-level chain: an instance of both, and of Object.
        "const g = new G(); g instanceof G && g instanceof F && g instanceof Object",
        "const c = Object.create(new F()); c instanceof F && c instanceof Object",
        "const c = Object.create(G.prototype); c instanceof G && c instanceof F",
        // A null-prototype object is an instance of nothing.
        "!(Object.create(null) instanceof Object)",
        // Arrays and the realm's Array constructor.
        "const A = [].constructor; [] instanceof A && [] instanceof Object && !(({}) instanceof A)",
        // Functions inherit from Function.prototype (clj-quickjs-102).
        "F instanceof Function && F instanceof Object && (() => 1) instanceof Function",
        "Object instanceof Function && Object.is instanceof Object && !(new F() instanceof Function)",
        "!(Object.create(F) instanceof F) && Object.create(F) instanceof Function",
        // The chain is read when the operator runs, by identity.
        "const o = new F(); F.prototype = {}; !(o instanceof F) && new F() instanceof F",
        "const p = {}; function H() {} H.prototype = p; Object.create(p) instanceof H",
        "const o = {}; Object.setPrototypeOf(o, F.prototype); o instanceof F",
        // A primitive left operand is false before `prototype` is read.
        "F.prototype = 1; !(1 instanceof F)",
        // Relational precedence: below `+`, above `==`, left-associative.
        "new F() instanceof F == true",
        "(0 + 1 instanceof F) === false",
        "new F() instanceof F === true",
        "(new F() instanceof F instanceof Object) === false",
    ] {
        assert_eq!(eval(&format!("{f} {src}")), ValueView::Bool(true), "{src}");
    }
    // The upstream `test_op2` case (`research/quickjs/tests/test_language.js`,
    // revision 7d1c13c5d8f8), less its `String` row.
    assert_eq!(
        eval("let a = {}; a instanceof Object"),
        ValueView::Bool(true)
    );
    // The right operand is evaluated before the check, the left one first.
    assert_eq!(
        string(
            "let log = ''; function l() { log = log + 'l'; return {}; } \
             function r() { log = log + 'r'; return 1; } \
             try { l() instanceof r(); } catch (e) { log = log + 'e'; } log"
        ),
        "lre"
    );
    // Catchable TypeErrors, reported at the `instanceof` token.
    for (src, message) in [
        ("({}) instanceof 1", "invalid 'instanceof' right operand"),
        ("({}) instanceof {}", "invalid 'instanceof' right operand"),
        (
            "1 instanceof undefined",
            "invalid 'instanceof' right operand",
        ),
        (
            "({}) instanceof __rt.vector()",
            "invalid 'instanceof' right operand",
        ),
        (
            "function F() {} F.prototype = 1; ({}) instanceof F",
            "operand 'prototype' property is not an object",
        ),
        // Callable without a `prototype`: an arrow function, a built-in
        // method (it would inherit none from Function.prototype).
        (
            "const a = () => 1; ({}) instanceof a",
            "operand 'prototype' property is not an object",
        ),
        (
            "({}) instanceof Object.is",
            "operand 'prototype' property is not an object",
        ),
    ] {
        assert_eq!(
            string(&format!(
                "let r = 'none'; try {{ {src}; }} catch (e) {{ r = 'caught'; }} r"
            )),
            "caught",
            "{src}"
        );
        let (kind, got, line, column) = failure_at(&format!("\n{src};"));
        assert_eq!(kind, VmErrorKind::TypeError, "{src}");
        assert_eq!(got, message, "{src}");
        let at = src.rfind("instanceof").unwrap() as u32 + 1;
        assert_eq!((line, column), (2, at), "{src}");
    }
    // The pellet's `new Object() instanceof F` row: `new Object()` is
    // itself a TypeError until `Object` is a constructor (clj-quickjs-71).
    assert_eq!(
        failure_at("function F() {} F.prototype = 1; new Object() instanceof F").0,
        VmErrorKind::TypeError
    );
    // An error object on the chain needs Error.prototype
    // (clj-quickjs-230): an explicit UnsupportedOperand, as for
    // Object.getPrototypeOf.
    let src = "let e; try { nope; } catch (x) { e = x; } e instanceof Object";
    assert_eq!(
        failure(src).kind(),
        VmErrorKind::UnsupportedOperand,
        "{src}"
    );
    // `instanceof` compiles to the `instanceof` opcode.
    assert!(opcodes(&exe(&format!("{f} new F() instanceof F"))).contains(&Opcode::Instanceof));
    // The `docs/grammar.md` example lines, each true.
    for src in [
        "function F() {} function G() {} G.prototype = new F(); new G() instanceof F && new G() instanceof G && !(new F() instanceof G);",
        "function F() {} ({}) instanceof Object && !(1 instanceof F) && !(Object.create(null) instanceof Object) && Object.create(F.prototype) instanceof F;",
        "let r = false; try { ({}) instanceof {}; } catch (e) { r = true; } r;",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
}

/// The `in` operator (clj-quickjs-74): HasProperty with ToPropertyKey of the
/// left operand, after QuickJS `OP_in` / `js_operator_in`. The `"x" in a`
/// cases are adapted from `test_op2` in
/// `research/quickjs/tests/test_language.js`, revision 7d1c13c5d8f8
/// (QuickJS copyright and MIT license notice: see
/// `loose_equality_with_a_boolean_left_operand`).
#[test]
fn in_operator() {
    for src in [
        // The pellet's acceptance rows.
        "'x' in {x: 2}",
        "!('y' in {x: 2})",
        "'x' in {x: undefined}",
        "'toString' in {}",
        "1 in {1: 0}",
        // ToPropertyKey of the left operand.
        "'1' in {1: 0} && 1 in {'1': 0}",
        "1.5 in {'1.5': 0} && -0 in {0: 0} && 1e21 in {'1e+21': 0}",
        "true in {true: 1} && null in {null: 1} && undefined in {undefined: 1}",
        "!(2 in {1: 0})",
        // Inherited properties, through any chain; the value does not matter.
        "const p = {a: undefined}; 'a' in Object.create(p) && !('a' in Object.create(null))",
        "!('toString' in Object.create(null))",
        "const o = {}; Object.setPrototypeOf(o, {q: 0}); 'q' in o",
        "'valueOf' in [] && 'constructor' in {} && 'constructor' in []",
        // `Object.prototype.__proto__` is inherited, not an own property.
        "'__proto__' in {} && !('__proto__' in Object.create(null))",
        // ... without running its getter, which would fail on a function.
        "function f() {} '__proto__' in f",
        // Arrays: elements, holes, `length`.
        "0 in [5] && !(1 in [5]) && !(1 in [1, , 3]) && 2 in [1, , 3] && 'length' in []",
        "'0' in ['a'] && !(-1 in ['a'])",
        // Functions: own properties, the lazy `prototype`, Object.prototype.
        "function f() {} 'prototype' in f && 'valueOf' in f && !('x' in f)",
        "const o = {m: function () {}}; 'prototype' in o.m",
        "'is' in Object",
        // Properties appear with the object's writes.
        "const o = {}; const before = 'k' in o; o.k = 0; !before && 'k' in o",
        // Persistent collections and builders have no JavaScript properties.
        "!(0 in __rt.vector(1)) && !('x' in __rt.vector()) && !('count' in __rt.vector())",
        // Relational precedence: below `+`, above `==`, left-associative.
        "'ab' in {ab: 1} === true",
        "('a' + 'b' in {ab: 1}) === true",
        "('x' in {x: 1} in {true: 1}) === true",
        "('x' in {} < 1) === true",
        // The `[In]` grammar: allowed in a `for` head's test and nested init.
        "let n = 0; for (let i = ('x' in {x: 1}) ? 0 : 5; i < 3 && 'x' in {x: 1}; i++) n++; n === 3",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
    // The upstream `test_op2` case (`research/quickjs/tests/test_language.js`,
    // revision 7d1c13c5d8f8).
    assert_eq!(
        eval("let a = {x : 2}; ('x' in a) === true && ('y' in a) === false"),
        ValueView::Bool(true)
    );
    // The left operand is evaluated first, then the right one, then the
    // right one is checked, then the key is converted.
    assert_eq!(
        string(
            "let log = ''; function l() { log = log + 'l'; return 'x'; } \
             function r() { log = log + 'r'; return 1; } \
             try { l() in r(); } catch (e) { log = log + 'e'; } log"
        ),
        "lre"
    );
    // An unbound left operand fails before the right one is looked at.
    assert_eq!(failure("nope in {};").kind(), VmErrorKind::ReferenceError);
    assert_eq!(failure("nope in 1;").kind(), VmErrorKind::ReferenceError);
    assert_eq!(failure("({}) in nope;").kind(), VmErrorKind::ReferenceError);
    // Catchable TypeErrors for a right operand that is not an object,
    // reported at the `in` token, before the key is converted.
    for (src, message) in [
        (
            "'x' in 1",
            "cannot use 'in' operator to search for 'x' in 1",
        ),
        (
            "'x' in null",
            "cannot use 'in' operator to search for 'x' in null",
        ),
        (
            "'x' in undefined",
            "cannot use 'in' operator to search for 'x' in undefined",
        ),
        (
            "'length' in 'abc'",
            "cannot use 'in' operator to search for 'length' in \"abc\"",
        ),
        (
            "0 in true",
            "cannot use 'in' operator to search for '0' in true",
        ),
        (
            "({}) in 1.5",
            "cannot use 'in' operator to search for an object key in 1.5",
        ),
    ] {
        assert_eq!(
            string(&format!(
                "let r = 'none'; try {{ {src}; }} catch (e) {{ r = 'caught'; }} r"
            )),
            "caught",
            "{src}"
        );
        let (kind, got, line, column) = failure_at(&format!("\n{src};"));
        assert_eq!(kind, VmErrorKind::TypeError, "{src}");
        assert_eq!(got, message, "{src}");
        let at = src.rfind(" in ").unwrap() as u32 + 2;
        assert_eq!((line, column), (2, at), "{src}");
    }
    // Functions and built-in functions inherit from Function.prototype
    // (clj-quickjs-102).
    for src in [
        "!('x' in Object) && 'call' in Object && 'apply' in Object.is",
        "'bind' in Object",
        "function f() {} 'call' in f && 'constructor' in f && 'valueOf' in f && !('x' in f)",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
    assert_eq!(eval("({}) in {}"), ValueView::Bool(false));
    // Not supported yet: a name Function.prototype does not have yet
    // (clj-quickjs-103 and later); an error object's properties
    // (clj-quickjs-230).
    for src in [
        "function f() {} 'toString' in f",
        "let e; try { nope; } catch (x) { e = x; } 'message' in e",
    ] {
        assert_eq!(
            failure(src).kind(),
            VmErrorKind::UnsupportedOperand,
            "{src}"
        );
    }
    // `in` compiles to the `in` opcode.
    assert!(opcodes(&exe("'x' in {}")).contains(&Opcode::In));
    // The `docs/grammar.md` example lines, each true.
    for src in [
        "'x' in {x: undefined} && 'toString' in {} && !('y' in {x: 2}) && 1 in {1: 0};",
        "0 in [1] && !(1 in [1]) && !('toString' in Object.create(null));",
        "let r = false; try { 'x' in 1; } catch (e) { r = true; } r;",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
}

/// `Executable::to_spec` re-encodes the argument counts of `construct` and
/// `call_method` (clj-quickjs-71): the reloaded script has the same bytes
/// and computes the same value.
#[test]
fn construct_and_method_call_operands_survive_a_spec_round_trip() {
    let script = exe("function F(a, b, c) { this.s = a * 100 + b * 10 + c; } \
         const o = {m: function (x, y) { return x - y; }}; \
         new F(1, 2, 3).s + o.m(9, 4) + Object.is(1, 1)");
    let spec = script.to_spec();
    let ops = decode(&spec.code);
    assert!(ops.iter().any(|&(_, op)| op == Opcode::Construct));
    assert!(ops.iter().any(|&(_, op)| op == Opcode::CallMethod));
    let operand = |op: Opcode| {
        let (at, _) = *ops.iter().find(|&&(_, o)| o == op).unwrap();
        u16::from_le_bytes([spec.code[at + 1], spec.code[at + 2]])
    };
    assert_eq!(operand(Opcode::Construct), 3);
    assert_eq!(operand(Opcode::CallMethod), 2);
    let again = Executable::load(spec.clone()).unwrap();
    assert_eq!(again.to_spec().code, spec.code);
    match run(&again, &[]) {
        RunOutcome::Complete(v) => assert_eq!(v.view(), ValueView::Number(129.0)),
        other => panic!("{other:?}"),
    }
}

/// `new` near the parser's nesting budget (clj-quickjs-71): 84 nested
/// `new`s and 60 constructions nested in arguments compile on the same
/// 2 MiB thread stack as `deep_expressions_compile_within_a_small_stack`.
#[test]
fn deep_new_expressions_compile_within_a_small_stack() {
    let news = format!("{}F;", "new ".repeat(84));
    let args = format!("{}1{};", "new F(".repeat(60), ")".repeat(60));
    let handle = std::thread::Builder::new()
        .stack_size(2 << 20)
        .spawn(move || {
            let a = compile_script("news", &news).map(|_| ());
            let b = compile_script("args", &args).map(|_| ());
            (a, b)
        })
        .unwrap();
    let (a, b) = handle.join().unwrap();
    a.unwrap();
    b.unwrap();
}

/// A constructor that waits on a pending cell (clj-quickjs-71): its frame,
/// whose `this` is the new object, is task-owned state, so the task
/// suspends at `__rt.need`, survives a collection, and completes with the
/// constructed object after the cell is published.
#[test]
fn a_constructor_suspends_at_a_need_and_completes_after_publication() {
    let rt = Runtime::new();
    let script = exe("function F(a) { this.a = a; this.v = __rt.need(5, 3); } \
         const o = new F(4); \
         o.constructor === F ? o.a * 100 + o.v : -1");
    let task = rt.spawn(&script, &[]).unwrap();
    let key = DependencyKey::new(rt.world(), DependencyId::new(5), ResultKind::new(3));
    match task.run().unwrap() {
        RunOutcome::Waiting(k) => assert_eq!(k, key),
        other => panic!("expected Waiting, got {other:?}"),
    }
    let mut requests = task.take_producer_requests();
    assert_eq!(requests.len(), 1);
    let request = requests.pop().unwrap();
    rt.collect().unwrap();
    let value = rt.root(Primitive::Number(7.0));
    let wakeups = rt
        .complete(request.key(), request.producer(), Completion::Ready(&value))
        .unwrap();
    let mut runnable = wakeups.into_runnable();
    assert_eq!(runnable.len(), 1);
    match runnable.pop().unwrap().run().unwrap() {
        RunOutcome::Complete(v) => assert_eq!(v.view(), ValueView::Number(407.0)),
        other => panic!("expected Complete, got {other:?}"),
    }
    // The task's completion value is the constructed object itself (the
    // cell is complete now: no wait).
    let direct = exe("function F() { this.v = __rt.need(5, 3); } new F()");
    match rt.spawn(&direct, &[]).unwrap().run().unwrap() {
        RunOutcome::Complete(v) => assert_eq!(v.view(), ValueView::Object),
        other => panic!("expected Complete, got {other:?}"),
    }
}

/// The values of the runtime that are not ordinary objects
/// (clj-quickjs-114, `crate::vm::property`): persistent collections and
/// builders have no JavaScript properties (reads give `undefined`, writes
/// throw a TypeError); function objects, error objects, a string's own
/// properties and boolean reads fail explicitly until their properties
/// exist (numbers and strings read through their prototypes since
/// clj-quickjs-73, `methods_on_primitive_bases`).
#[test]
fn property_access_on_values_that_are_not_ordinary_objects() {
    assert_eq!(
        eval("let f = function () {}; typeof f.bind === 'function';"),
        ValueView::Bool(true)
    );
    assert_eq!(eval("__rt.vector(1, 2).x"), ValueView::Undefined);
    assert_eq!(eval("__rt.map(1, 2).x"), ValueView::Undefined);
    assert_eq!(eval("__rt.vectorBuilder().x"), ValueView::Undefined);
    for base in ["__rt.map()", "__rt.mapBuilder()", "__rt.vectorBuilder()"] {
        let (kind, message, ..) = failure_at(&format!("let b = {base};\nb.x = 1;"));
        assert_eq!(kind, VmErrorKind::TypeError, "{base}: {message}");
    }
    // Occurrences and atoms, handed to a function by the embedder, behave
    // the same: reads give undefined, writes throw a catchable TypeError.
    let rt = Runtime::new();
    let one = rt.root(Primitive::Number(1.0));
    let occurrence = rt.scope(|s| {
        let at = clj_quickjs::Provenance {
            source: 1,
            line: 1,
            column: 1,
        };
        let o = s
            .new_occurrence(s.primitive(Primitive::Number(1.0)), at)
            .unwrap();
        s.persist(o)
    });
    let atom = rt.new_atom(&one).unwrap();
    let probe = compile_function(
        "function f(v) { let r = v.x === undefined ? 1 : 0; \
         try { v.x = 1; } catch (e) { r = r + 10; } return r; }",
    )
    .unwrap();
    for base in [&occurrence, &atom] {
        let task = rt.spawn(&probe, &[base]).unwrap();
        match task.run().unwrap() {
            RunOutcome::Complete(v) => assert_eq!(v.view(), ValueView::Number(11.0)),
            other => panic!("expected Complete, got {other:?}"),
        }
    }
    assert_eq!(
        string("let r = 'none'; try { __rt.vector().x = 1; } catch (e) { r = 'caught'; } r"),
        "caught"
    );
    assert_eq!(
        failure_at("let v = __rt.vector();\nv.x = 1;"),
        (
            VmErrorKind::TypeError,
            "cannot set property 'x' of a vector: it has no JavaScript properties".to_string(),
            2,
            2
        )
    );
    for (src, needle) in [
        ("let s = 'a';\ns.length;", "of a string"),
        ("let b = true;\nb.x;", "of a boolean"),
        (
            "let e; try { null.x; } catch (c) { e = c; }\ne.message;",
            "of an error object",
        ),
    ] {
        let (kind, message, line, _) = failure_at(src);
        assert_eq!(kind, VmErrorKind::UnsupportedOperand, "{src}: {message}");
        assert!(message.contains(needle), "{src}: {message}");
        assert_eq!(line, 2, "{src}");
    }
    // An unsupported operand is not catchable: no JavaScript sees a wrong
    // answer.
    let (kind, ..) = failure_at("let r; try { true.x; } catch (e) { r = 1; } r");
    assert_eq!(kind, VmErrorKind::UnsupportedOperand);
}

/// `Object.prototype`, the `Object` constructor and `Object.is` as realm
/// intrinsics, and method calls passing their receiver (clj-quickjs-117;
/// `docs/architecture.md`, "Realm intrinsics"). The `a = {};
/// a.toString()` row follows `test` (line 124) of
/// `research/quickjs/tests/test_builtin.js`, and the `Object.is` rows the
/// `assert` helper (line 1) of `research/quickjs/tests/test_language.js`,
/// revision 7d1c13c5d8f8 (restated as Rust assertions); the QuickJS MIT
/// notice on `object_literals_and_named_properties` covers them. The rest
/// are synthetic fixtures, not light-cljs output.
#[test]
fn object_intrinsics() {
    // The pellet's acceptance rows.
    assert_eq!(string("({}).toString()"), "[object Object]");
    assert_eq!(string("let a = {}; a.toString()"), "[object Object]");
    assert_eq!(eval("let o = {}; o.valueOf() === o"), ValueView::Bool(true));
    assert_eq!(
        eval("({}).toString === Object.prototype.toString"),
        ValueView::Bool(true)
    );
    assert_eq!(num("({toString: 1}).toString"), 1.0);
    for (src, expected) in [
        ("Object.is(NaN, NaN)", true),
        ("Object.is(0, -0)", false),
        ("let o = {}; Object.is(o, o)", true),
        ("let o = {}; Object.is(o, {})", false),
        ("Object() !== Object()", true),
        ("let o = {}; Object(o) === o", true),
        // SameValue on the other value kinds; missing arguments are
        // undefined.
        ("Object.is('ab', 'a' + 'b')", true),
        ("Object.is(null, undefined)", false),
        ("Object.is()", true),
        ("Object.is(-0, -0)", true),
        ("Object.is(1, '1')", false),
        // `Object()` of undefined, null, or any object.
        ("Object(undefined) !== Object(undefined)", true),
        ("let f = () => 1; Object(f) === f", true),
        ("let v = __rt.vector(); Object(v) === v", true),
        ("Object(Object) === Object", true),
    ] {
        assert_eq!(eval(src), ValueView::Bool(expected), "{src}");
    }
    // The `docs/grammar.md` example lines, each true.
    for src in [
        r#"({}).toString() === "[object Object]" && ({}).toString === Object.prototype.toString;"#,
        r#"Object.is(NaN, NaN) && !Object.is(0, -0) && Object() !== Object() && typeof Object === "function";"#,
        r#"let o = {toString: () => "own"}; o.toString() === "own" && Object(o) === o && o.valueOf() === o;"#,
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
    assert_eq!(string("Object(null).toString()"), "[object Object]");
    assert_eq!(string("Object().toString()"), "[object Object]");
    // clj-quickjs-69 has landed.
    assert_eq!(string("typeof Object"), "function");
    assert_eq!(string("typeof Object.is"), "function");
    assert_eq!(string("typeof Object.prototype"), "object");
    assert_eq!(string("typeof Object.prototype.toString"), "function");

    // The intrinsics' own properties.
    assert_eq!(num("Object.length"), 1.0);
    assert_eq!(num("Object.is.length"), 2.0);
    assert_eq!(num("Object.prototype.toString.length"), 0.0);
    assert_eq!(string("Object.name"), "Object");
    assert_eq!(string("Object.prototype.valueOf.name"), "valueOf");
    assert_eq!(
        eval("Object.prototype.constructor === Object"),
        ValueView::Bool(true)
    );
    assert_eq!(eval("({}).constructor === Object"), ValueView::Bool(true));
    assert_eq!(
        eval("Object(null).valueOf === Object.prototype.valueOf"),
        ValueView::Bool(true)
    );
    // `Object.prototype` itself has a null prototype.
    assert_eq!(eval("Object.prototype.missing"), ValueView::Undefined);
    assert_eq!(eval("({}).missing"), ValueView::Undefined);
    // Lexical bindings shadow the built-in global.
    assert_eq!(num("let Object = 1; Object"), 1.0);
    assert_eq!(num("function f(Object) { return Object; } f(2)"), 2.0);
    assert_eq!(string("{ const Object = 'c'; } typeof Object"), "function");

    // An own property shadows an inherited one; a write adds an own
    // property (the inherited one is writable) and leaves
    // `Object.prototype` alone.
    assert_eq!(
        string("let o = {}; o.toString = function () { return 'mine'; }; o.toString()"),
        "mine"
    );
    assert_eq!(
        string("let o = {}; o.toString = 1; let p = {}; p.toString() + ':' + typeof o.toString"),
        "[object Object]:number"
    );
    assert_eq!(num("let o = Object(); o.x = 2; o.x"), 2.0);
    assert_eq!(
        string("let o = {}; o.constructor = 'c'; o.constructor"),
        "c"
    );

    // The receiver: a plain call passes `undefined` (strict mode); a method
    // call passes its base, evaluated once; a parenthesized member callee
    // keeps it, a comma expression does not.
    assert_eq!(
        string("let f = Object.prototype.toString; f()"),
        "[object Undefined]"
    );
    assert_eq!(string("let o = {}; (o.toString)()"), "[object Object]");
    assert_eq!(
        string("let o = {}; (0, o.toString)()"),
        "[object Undefined]"
    );
    assert_eq!(string("Object.prototype.toString()"), "[object Object]");
    assert_eq!(
        string("let o = {f: function () {}}; Object.prototype.toString === o.f ? 'same' : 'other'"),
        "other"
    );
    assert_eq!(
        num("let n = 0; function g() { n = n + 1; return {}; } g().toString(); n"),
        1.0
    );
    assert_eq!(string("({inner: {}}).inner.toString()"), "[object Object]");
    assert_eq!(
        string("let o = {f: Object.prototype.valueOf}; o.f() === o ? 'same' : 'other'"),
        "same"
    );
    assert_eq!(
        string("let o = {f: Object.prototype.toString}; o.f()"),
        "[object Object]"
    );
    // A method that is a JavaScript function gets its arguments (and its
    // receiver as `this`: `method_calls_bind_this`).
    assert_eq!(
        num("let o = {m: function (a, b) { return a - b; }}; o.m(5, 2)"),
        3.0
    );
    assert_eq!(num("let o = {m: (x) => x * 2}; o.m(21)"), 42.0);
    // A built-in as the callback of a resumable native.
    assert_eq!(
        eval("__rt.reduce(Object.is, 1, __rt.vector(1))"),
        ValueView::Bool(true)
    );
    // The instructions: a literal inherits through `new_object_literal`,
    // `Object` is `get_builtin_global`, a method call is `call_method`.
    let ops = opcodes(&exe("let o = {}; o.toString() + Object.is(o, o)"));
    for op in [
        Opcode::NewObjectLiteral,
        Opcode::CallMethod,
        Opcode::GetBuiltinGlobal,
    ] {
        assert!(ops.contains(&op), "{op:?} missing from {ops:?}");
    }
    assert!(!ops.contains(&Opcode::NewObject), "{ops:?}");
}

/// What the intrinsics reject (clj-quickjs-117): writes to built-in objects
/// are catchable `TypeError`s (they are shared and immutable), assignment
/// to a built-in global is a compile-time `Unsupported` error until the
/// global object exists (clj-quickjs-104), and what needs a later pellet
/// fails with `UnsupportedOperand` instead of a wrong value.
#[test]
fn object_intrinsics_reject_what_they_do_not_support() {
    for src in [
        "Object.prototype.x = 1;",
        "Object.prototype.toString = 1;",
        "Object.is = 1;",
        "Object.prototype.toString.x = 1;",
        "Object.prototype = {};",
    ] {
        let caught = format!("let r = 'none'; try {{ {src} }} catch (e) {{ r = 'caught'; }} r");
        assert_eq!(string(&caught), "caught", "{src}");
        let (kind, message, ..) = failure_at(src);
        assert_eq!(kind, VmErrorKind::TypeError, "{src}: {message}");
        assert!(message.contains("built-in objects are shared"), "{message}");
    }
    // Nothing changed.
    assert_eq!(
        eval("try { Object.prototype.x = 1; } catch (e) {} ({}).x"),
        ValueView::Undefined
    );
    assert_eq!(
        failure_at("let o = {};\nObject.prototype.y = o;"),
        (
            VmErrorKind::TypeError,
            "cannot set property 'y' of a built-in object: built-in objects are shared by all tasks and immutable".to_string(),
            2,
            17
        )
    );
    // `undefined`/`null` receivers of valueOf.
    let (kind, message, ..) = failure_at("let f = Object.prototype.valueOf; f()");
    assert_eq!(
        (kind, message.as_str()),
        (VmErrorKind::TypeError, "cannot convert undefined to object")
    );
    assert_eq!(
        string(
            "let f = Object.prototype.valueOf; let r = 'none'; try { f(); } catch (e) { r = 'caught'; } r"
        ),
        "caught"
    );
    // Wrapper objects (clj-quickjs-72), missing Function.prototype methods
    // (clj-quickjs-102) and the error tag (clj-quickjs-230).
    for (src, needle) in [
        ("Object(1)", "wrapper object"),
        ("Object('s')", "wrapper object"),
        ("Object(true)", "wrapper object"),
        ("Object.is.toString()", "Function.prototype"),
    ] {
        let (kind, message, ..) = failure_at(src);
        assert_eq!(kind, VmErrorKind::UnsupportedOperand, "{src}: {message}");
        assert!(message.contains(needle), "{src}: {message}");
    }
    // Assigning a built-in global waits for the global object.
    for src in ["Object = 1;", "Object += 1;", "Object++;"] {
        let e = compile_script("test", src).unwrap_err();
        assert_eq!(e.kind, SyntaxErrorKind::Unsupported, "{src}: {e}");
        assert!(e.message.contains("`Object`"), "{src}: {e}");
    }
    assert_eq!(num("let Object = 1; Object = 2; Object"), 2.0);
}

/// Computed member access `o[k]` and `o[k] = v` on ordinary objects
/// (clj-quickjs-115; the instructions came with arrays, clj-quickjs-118):
/// the key goes through ToPropertyKey, so a number and its Number::toString
/// name the same property, and the base, the key and the right-hand side
/// evaluate in that order. Synthetic fixtures, not light-cljs output.
#[test]
fn computed_member_access() {
    // The acceptance rows, one script: each key reads back under its
    // canonical name.
    assert_eq!(
        num("let o = {}; let r = 0; \
             o[\"x\"] = 1; r = r * 10 + o.x; \
             o[1] = 2; r = r * 10 + o[\"1\"]; \
             o[1.5] = 3; r = r * 10 + o[\"1.5\"]; \
             o[-0] = 4; r = r * 10 + o[\"0\"]; \
             o[true] = 5; r = r * 10 + o.true; \
             o[undefined] = 6; r = r * 10 + o.undefined; \
             let k = \"y\"; o[k] = 7; r = r * 10 + o.y; \
             r"),
        1234567.0
    );
    // ... and the other way round: a name reads back under a computed key.
    for (src, expected) in [
        ("let o = {x: 1}; o['x']", 1.0),
        ("let o = {}; o['1'] = 2; o[1]", 2.0),
        ("let o = {}; o.true = 3; o[true]", 3.0),
        ("let o = {}; o['1.5'] = 4; o[3 / 2]", 4.0),
        ("let o = {}; o[0] = 5; o[-0]", 5.0),
        ("let o = {}; o[false] = 6; o.false", 6.0),
        ("let o = {}; o[null] = 7; o.null", 7.0),
        ("let o = {}; o[0 / 0] = 8; o.NaN", 8.0),
        ("let o = {}; o[1 / 0] = 9; o['Infinity']", 9.0),
        ("let o = {}; o[1e21] = 10; o['1e+21']", 10.0),
        ("let o = {}; o[0.1] = 11; o['0.1']", 11.0),
        ("let o = {}; o[-1] = 12; o['-1']", 12.0),
        ("let o = {}; o['a' + 'b'] = 13; o.ab", 13.0),
        // A comma key evaluates both and uses the last.
        ("let o = {x: 14}; o[(0, 'x')]", 14.0),
        // The value of an assignment is the assigned value.
        ("let o = {}; (o['z'] = 15) + o.z", 30.0),
    ] {
        assert_eq!(num(src), expected, "{src}");
    }
    // Distinct strings are distinct keys, even when they denote one number.
    assert_eq!(
        eval("let o = {}; o['01'] = 1; o[1] === undefined && o['01'] === 1"),
        ValueView::Bool(true)
    );
    // Missing keys read undefined; inherited ones read along the chain;
    // an own computed write shadows an inherited property.
    for src in [
        "({})['nope'] === undefined",
        "({})[7] === undefined",
        "({})['toString'] === Object.prototype.toString",
        "let o = {}; o['toString'] = 1; o.toString === 1 && ({}).toString !== 1",
        "let o = {m: function () { return this; }}; let k = 'm'; o[k]() === o",
        // Chains: `a[b][c]`, `a.b[c].d`, `a[b](c)`.
        "let o = {p: {q: 3}}; o['p']['q'] === 3",
        "let o = {a: {b: {c: 4}}}; let k = 'b'; o.a[k].c === 4",
        "let o = {f: function (n) { return n + 1; }}; o['f'](1) === 2",
        "let o = {}; o['p'] = {}; o['p']['q'] = 5; o.p.q === 5",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
    // The `docs/grammar.md` example lines, each true.
    for line in [
        "let o = {}; o[1] = \"one\"; o[\"1\"] === \"one\" && o[1.5] === undefined;",
        "let p = {}; p[-0] = 0; p[true] = 1; p[\"0\"] === 0 && p.true === 1 && p[undefined] === undefined;",
        "let q = {}; let k = \"y\"; q[k] = 7; q.y === 7 && ({})[\"toString\"] === Object.prototype.toString;",
    ] {
        assert_eq!(eval(line), ValueView::Bool(true), "{line}");
    }

    // `null[0]` throws a TypeError, caught by `catch`.
    assert_eq!(
        string("let r = 'none'; try { null[0]; } catch (e) { r = 'caught'; } r"),
        "caught"
    );
    assert_eq!(
        failure_at("null[0];"),
        (
            VmErrorKind::TypeError,
            "cannot read property '0' of null".to_string(),
            1,
            5
        )
    );
    assert_eq!(
        failure_at("let u;\nu['k'] = 1;"),
        (
            VmErrorKind::TypeError,
            "cannot set property 'k' of undefined".to_string(),
            2,
            2
        )
    );

    // Order: the base, then the key, then the right-hand side, each once;
    // the log is captured by the closures.
    assert_eq!(
        string(
            "function run() { \
               let log = ''; let o = {}; \
               let f = () => { log = log + 'f'; return o; }; \
               let g = () => { log = log + 'g'; return 'k'; }; \
               let h = () => { log = log + 'h'; return 1; }; \
               f()[g()] = h(); \
               return log + o.k; \
             } run()"
        ),
        "fgh1"
    );
    assert_eq!(
        string(
            "function run() { \
               let log = ''; let o = {k: 2}; \
               let f = () => { log = log + 'f'; return o; }; \
               let g = () => { log = log + 'g'; return 'k'; }; \
               return log + f()[g()] + log; \
             } run()"
        ),
        "2fg"
    );

    assert_eq!(eval("let o = {}; o[{}];"), ValueView::Undefined);
    assert_eq!(num("let o = {}; o[o] = 1; o['[object Object]']"), 1.0);
    assert!(num("let o = {}; o[o]++; o['[object Object]']").is_nan());
    assert!(num("let o = {}; o[o] += 1; o['[object Object]']").is_nan());
}

/// Function own properties and the lazy `prototype` object
/// (clj-quickjs-125; `docs/architecture.md`, "Function own properties").
/// The `f.prototype.constructor === f` row and the `g` rows (the lazy
/// prototype bug note) follow `test_prototype` (line 260) of
/// `research/quickjs/tests/test_language.js`, revision 7d1c13c5d8f8
/// (restated as Rust assertions); the QuickJS MIT notice on
/// `loose_equality_with_a_boolean_left_operand` covers them. The rest are
/// synthetic fixtures, not light-cljs output.
#[test]
fn function_own_properties() {
    let t = ValueView::Bool(true);
    for src in [
        // Upstream `test_prototype`.
        "let f = function f() {}; f.prototype.constructor === f",
        "let f = function f() {}; f.prototype === f.prototype",
        // Declarations too; a read inside the function sees the same object.
        "function h() {} h.prototype.constructor === h",
        "function g() { return g.prototype; } g() === g.prototype",
        // Each evaluation of a function expression has its own prototype.
        "function mk() { return function () {}; } let a = mk(), b = mk(); \
         a.prototype !== b.prototype && a.prototype.constructor === a",
        // The prototype object is an ordinary object inheriting from
        // Object.prototype, owned by the task, so it takes methods.
        "let f = function () {}; f.prototype.toString() === '[object Object]'",
        "let f = function () {}; f.prototype.m = function () { return this === f.prototype; }; \
         f.prototype.m()",
        // Ordinary properties: absent until assigned, then own and writable;
        // a method call on a function passes it as the receiver.
        "let f = function () {}; f.y === undefined",
        "let f = function () {}; f.m = function () { return this === f; }; f.m()",
        // Names other than Function.prototype's continue at Object.prototype,
        // also before anything else built the realm.
        "let f = function () {}; typeof f.valueOf === 'function'",
        "let f = function () {}; f.valueOf === Object.prototype.valueOf",
        // Arrow functions have no `prototype` but take properties.
        "(() => 1).prototype === undefined",
        "let a = () => 1; a.prototype = 3; a.x = 4; a.prototype + a.x === 7",
    ] {
        assert_eq!(eval(src), t, "{src}");
    }
    assert_eq!(num("let f = function f() {}; f.x = 1; f.x"), 1.0);
    assert_eq!(
        num("let f = function () {}; f.x = 1; f.x = f.x + 1; f.x"),
        2.0
    );
    // `prototype` is writable, before or after it was first read.
    assert_eq!(
        num("let f = function () {}; f.prototype = 5; f.prototype"),
        5.0
    );
    assert_eq!(
        num("let f = function () {}; let p = f.prototype; f.prototype = 5; f.prototype"),
        5.0
    );
    assert_eq!(
        eval("let f = function () {}; let p = f.prototype; f.prototype = 5; p.constructor === f"),
        t
    );
    // Properties survive budget yields between their writes and reads.
    let src = "let f = function () {}; f.prototype.n = 0; let i = 0;\n\
               while (i < 50) { f.prototype.n = f.prototype.n + i; i = i + 1; }\n\
               f.prototype.n + (f.prototype.constructor === f ? 1000 : 0)";
    let rt = Runtime::new();
    let task = rt.spawn(&exe(src), &[]).unwrap();
    let mut yields = 0;
    let result = loop {
        match task
            .run_with(Budget::instructions(5), &mut NoEffects)
            .unwrap()
        {
            RunOutcome::Yielded => yields += 1,
            RunOutcome::Complete(v) => break v.view(),
            other => panic!("unexpected {other:?}"),
        }
    };
    assert_eq!(result, ValueView::Number(2225.0));
    assert!(yields > 10, "only {yields} yields");

    // `call`, `apply` and `constructor` are inherited from
    // Function.prototype (clj-quickjs-102).
    for src in [
        "let f = function () {}; f.call === Function.prototype.call && \
         f.apply === Function.prototype.apply && f.constructor === Function",
        "let f = () => 1; f.call === Function.prototype.call",
    ] {
        assert_eq!(eval(src), t, "{src}");
    }
    assert_eq!(num("let f = function () {};\nf.length;"), 0.0);
    assert_eq!(string("let f = function f() {};\nf.name;"), "f");
    assert_eq!(
        failure_at("let f = function () {};\nf.length = 1;").0,
        VmErrorKind::TypeError
    );
    // Function.prototype's other methods do not exist yet: their names fail
    // explicitly, not with a wrong `undefined`, and the failure is not
    // catchable.
    for src in [
        "let f = function () {};\nf.toString();",
        "let f = function () {};\nf.caller;",
        "let o = Object.create(function () {});\no.toString;",
    ] {
        let (kind, message, line, _) = failure_at(src);
        assert_eq!(kind, VmErrorKind::UnsupportedOperand, "{src}: {message}");
        assert!(message.contains("Function.prototype"), "{src}: {message}");
        assert_eq!(line, 2, "{src}");
    }
    // `bind` is now inherited on all callable kinds (clj-quickjs-103).
    for src in [
        "typeof Object.bind === 'function'",
        "typeof Object.prototype.toString.bind === 'function'",
        "let f = function () {}; f.bind === Function.prototype.bind",
        "let f = () => 1; f.bind === Function.prototype.bind",
        "let r = true; try { r = typeof (function () {}).bind === 'function'; } catch (e) { r = false; } r",
        "let g = Object.is; g.bind === Function.prototype.bind",
        "let p = Function.prototype; typeof p.bind === 'function'",
    ] {
        assert_eq!(eval(src), t, "{src}");
    }

    // Attributes (with clj-quickjs-119). The upstream `g` rows: a
    // definition materializes `prototype` first, so the partial descriptor
    // keeps its value; afterwards an assignment is a TypeError.
    for src in [
        "function g() {} Object.defineProperty(g, 'prototype', { writable: false }); \
         g.prototype.constructor === g",
        "function g() {} Object.defineProperty(g, 'prototype', { writable: false }); \
         let r = false; try { g.prototype = {}; } catch (e) { r = true; } r",
        "let f = function f() {}; \
         Object.getOwnPropertyDescriptor(f, 'prototype').enumerable === false",
        // `prototype`: writable, not enumerable, not configurable, created
        // by the descriptor read itself.
        "let f = function () {}; let d = Object.getOwnPropertyDescriptor(f, 'prototype'); \
         d.writable && !d.enumerable && !d.configurable && d.value === f.prototype",
        // `constructor`: writable, not enumerable, configurable.
        "let f = function () {}; \
         let d = Object.getOwnPropertyDescriptor(f.prototype, 'constructor'); \
         d.writable && !d.enumerable && d.configurable && d.value === f",
        // Not configurable: redefining it otherwise is a TypeError.
        "let f = function () {}; let r = false; \
         try { Object.defineProperty(f, 'prototype', { enumerable: true }); } \
         catch (e) { r = true; } r",
        // Assigned properties are ordinary data properties; defined ones
        // follow their descriptor; arrows have no own `prototype`.
        "let f = function () {}; f.x = 1; let d = Object.getOwnPropertyDescriptor(f, 'x'); \
         d.writable && d.enumerable && d.configurable",
        "let f = () => 1; Object.defineProperty(f, 'k', { value: 2 }) === f && f.k === 2 \
         && !Object.getOwnPropertyDescriptor(f, 'k').writable",
        "Object.getOwnPropertyDescriptor(() => 1, 'prototype') === undefined",
        "Object.getOwnPropertyDescriptor(function () {}, 'x') === undefined",
    ] {
        assert_eq!(eval(src), t, "{src}");
    }
    // The caught failures above are TypeErrors.
    for src in [
        "function g() {} Object.defineProperty(g, 'prototype', { writable: false });\n\
         g.prototype = {};",
        "let f = function () {};\nObject.defineProperty(f, 'prototype', { enumerable: true });",
    ] {
        let (kind, message, line, _) = failure_at(src);
        assert_eq!(
            (kind, line),
            (VmErrorKind::TypeError, 2),
            "{src}: {message}"
        );
    }
    // A function's prototype is Function.prototype (clj-quickjs-102).
    let src = "let f = function f() {}; Object.getPrototypeOf(f) === Function.prototype";
    assert_eq!(eval(src), t, "{src}");
}

/// Array literals and array objects (clj-quickjs-118): elisions are holes
/// (absent elements), one final comma adds nothing, `length` follows the
/// greatest index written, elements are evaluated left to right, and every
/// literal is a new task-owned object. Computed member access `a[k]` is
/// covered here as far as arrays need it (on ordinary objects:
/// `computed_member_access`, clj-quickjs-115).
/// Arrays inherit from the realm intrinsic `Array.prototype`
/// (`Object.getPrototypeOf([])` is in `prototype_reflection`), which
/// inherits from `Object.prototype` (clj-quickjs-117).
/// Synthetic fixtures, not light-cljs output.
#[test]
fn array_literals() {
    assert_eq!(num("[1, 2, 3][2]"), 3.0);
    assert_eq!(num("[1, 2, 3].length"), 3.0);
    assert_eq!(num("[].length"), 0.0);
    assert_eq!(num("[1, 2,].length"), 2.0);
    assert_eq!(num("[ , ].length"), 1.0);
    assert_eq!(num("[,,].length"), 2.0);
    assert_eq!(num("[1, , 3].length"), 3.0);
    assert_eq!(eval("[1, , 3][1]"), ValueView::Undefined);
    assert_eq!(eval("[ , ][0]"), ValueView::Undefined);
    assert_eq!(eval("[1, 2][2]"), ValueView::Undefined);
    let a = "let a = [1]; a[3] = 4;";
    assert_eq!(num(&format!("{a} a.length")), 4.0);
    assert_eq!(num(&format!("{a} a[\"0\"]")), 1.0);
    assert_eq!(num(&format!("{a} a[1 + 2]")), 4.0);
    assert_eq!(eval(&format!("{a} a[2]")), ValueView::Undefined);
    // A write below the length leaves it; an assignment's value is the
    // assigned value.
    assert_eq!(
        num("let a = [1, 2, 3]; a[0] = 9; a.length * 10 + a[0]"),
        39.0
    );
    assert_eq!(num("let a = []; (a[0] = 5) + a[0]"), 10.0);
    // Identity: a literal is a new object each time; aliases share it.
    assert_eq!(eval("[] !== []"), ValueView::Bool(true));
    assert_eq!(eval("let a = []; a === a"), ValueView::Bool(true));
    assert_eq!(
        eval("let a = []; let b = a; b[0] = 1; a[0] === 1"),
        ValueView::Bool(true)
    );
    assert_eq!(
        num("function f() { return []; } let x = f(), y = f(); x[0] = 1; y.length"),
        0.0
    );
    assert_eq!(string("typeof []"), "object");
    // Elements of every kind, nested literals included.
    assert_eq!(string("[\"a\", \"b\"][1]"), "b");
    assert_eq!(num("[[1, 2], [3, 4]][1][0]"), 3.0);
    assert_eq!(num("[{x: 7}][0].x"), 7.0);
    assert_eq!(num("({a: [1, 2]}).a[1]"), 2.0);
    assert_eq!(
        num("let f = [function (n) { return n * 2; }][0]; f(21)"),
        42.0
    );
    assert_eq!(eval("[null, undefined][1]"), ValueView::Undefined);
    // The `docs/grammar.md` example lines, each true.
    for line in [
        "[1, 2, 3][2] === 3 && [1, 2,].length === 2 && [ , ].length === 1 && [].length === 0;",
        "[1, , 3].length === 3 && [1, , 3][1] === undefined && [] !== [] && typeof [] === \"object\";",
        "let a = [1]; a[3] = 4; a.length === 4 && a[\"0\"] === 1 && a[2] === undefined;",
    ] {
        assert_eq!(eval(line), ValueView::Bool(true), "{line}");
    }

    // Elements evaluate left to right; holes evaluate nothing.
    let log = "let log = 0; function f(n) { log = log * 10 + n; return n; }";
    assert_eq!(num(&format!("{log} [f(1), , f(2), f(3)]; log")), 123.0);
    assert_eq!(
        num(&format!(
            "{log} let a = [f(1), , f(2)]; log * 10 + a.length"
        )),
        123.0
    );
    // `g()[k()] = h()`: the base, the key, then the value.
    assert_eq!(
        num(&format!(
            "{log} let t = []; function g() {{ f(1); return t; }} \
             function k() {{ f(2); return 0; }} function h() {{ f(3); return 9; }} \
             g()[k()] = h(); log * 10 + t[0]"
        )),
        1239.0
    );

    // Keys: a number or string that is an array index names an element; any
    // other key is an ordinary property of the array and leaves `length`.
    let keys = [
        ("let a = [5]; a[-0]", 5.0),
        ("let a = [5]; a['0']", 5.0),
        ("let a = [5]; a[0.0]", 5.0),
        ("let a = []; a['2'] = 1; a.length", 3.0),
        ("let a = []; a[1.5] = 1; a.length * 10 + a['1.5']", 1.0),
        ("let a = []; a['01'] = 1; a.length * 10 + a['01']", 1.0),
        ("let a = []; a[-1] = 1; a.length * 10 + a['-1']", 1.0),
        ("let a = [3]; a.x = 2; a.length * 10 + a.x", 12.0),
        ("let a = [3]; a[true] = 2; a.length * 10 + a['true']", 12.0),
        ("let a = []; a[4294967294] = 1; a.length", 4294967295.0),
        ("let a = []; a[4294967295] = 1; a.length", 0.0),
    ];
    for (src, expected) in keys {
        assert_eq!(num(src), expected, "{src}");
    }
    assert_eq!(eval("let a = [5]; a['00']"), ValueView::Undefined);
    // Far writes are stored sparsely; holes in between read undefined.
    assert_eq!(
        num("let a = [1]; a[100000] = 2; a[50] === undefined ? a.length : -1"),
        100001.0
    );
    assert_eq!(
        num("let a = []; let i = 0; while (i < 3000) { a[i] = i; i = i + 1; } a[2999] + a.length"),
        5999.0
    );
    assert_eq!(
        num("let a = []; a[5000] = 1; a[0] = 2; a[5001] = 3; a[0] + a[5000] + a[5001] + a.length"),
        5008.0
    );
    // Computed access on ordinary objects names the same properties.
    assert_eq!(
        num("let o = {}; o['x'] = 1; o[1] = 2; o.x * 10 + o['1']"),
        12.0
    );
    assert_eq!(num("let o = {1: 4}; o[1]"), 4.0);
    // Persistent vectors are not JavaScript arrays: no elements, no length.
    assert_eq!(eval("__rt.vector(1, 2)[0]"), ValueView::Undefined);
    assert_eq!(eval("__rt.vector(1, 2).length"), ValueView::Undefined);
    // ... and an array is not a collection.
    let (kind, message, ..) = failure_at("__rt.count([1, 2]);");
    assert!(
        matches!(kind, VmErrorKind::Collection(_)) && message.contains("array"),
        "{kind:?}: {message}"
    );

    // Errors: a null/undefined base is a catchable TypeError at the `[`.
    assert_eq!(
        string("let r = 'none'; try { null[0]; } catch (e) { r = 'caught'; } r"),
        "caught"
    );
    assert_eq!(
        failure_at("let u;\nu[0];"),
        (
            VmErrorKind::TypeError,
            "cannot read property '0' of undefined".to_string(),
            2,
            2
        )
    );
    assert_eq!(
        failure_at("let n = null;\n  n['x'] = [];"),
        (
            VmErrorKind::TypeError,
            "cannot set property 'x' of null".to_string(),
            2,
            4
        )
    );
    // The right-hand side runs before the base is checked.
    assert_eq!(
        num("let hit = 0; try { null[0] = (hit = 1); } catch (e) {} hit"),
        1.0
    );
    // A null/undefined base is checked before the key is converted, so an
    // object key (which needs ToPrimitive) still gives the catchable
    // TypeError, after the key expression ran.
    for src in [
        "let r = 0; try { null[{}]; } catch (e) { r = 1; } r",
        "let r = 0; let u; try { u[[]] = 1; } catch (e) { r = 1; } r",
        "let r = 0; try { null[(r = 5, {})]; } catch (e) { r = r + 1; } r - 5",
    ] {
        assert_eq!(num(src), 1.0, "{src}");
    }
    assert_eq!(
        failure_at("let n = null;\n  n[{}];"),
        (
            VmErrorKind::TypeError,
            "cannot read a property of null".to_string(),
            2,
            4
        )
    );
    // Setting length truncates; an ordinary object key converts to a name.
    assert_eq!(num("let a = [1];\na.length = 0; a.length"), 0.0);
    assert_eq!(num("let a = [1]; a[{}] = 1; a['[object Object]']"), 1.0);
    let (kind, message, line, _) = failure_at("let a = [1];\na[[]];");
    assert_eq!((kind, line), (VmErrorKind::UnsupportedOperand, 2));
    assert!(message.contains("Array.prototype.toString"));
    // The prototype chain: Array.prototype, then Object.prototype. Its
    // push is supported; toString and the constructor fail
    // explicitly rather than answer as Object's would.
    for src in [
        "[].valueOf === Object.prototype.valueOf",
        "let a = [1]; a.valueOf() === a",
        "typeof [].push === 'function' && [].hasOwnProperty === undefined",
        "[].toString !== Object.prototype.toString && typeof [].toString === 'function'",
        "[].constructor !== Object && [].constructor.name === 'Array'",
        "[].constructor.prototype === [1].constructor.prototype",
        "[].constructor.length === 1 && [].length === 0",
        "Object.prototype.toString === ({}).toString",
        // Object.prototype.toString itself tags an array receiver `Array`.
        "let a = []; a.tag = Object.prototype.toString; a.tag() === '[object Array]'",
        "let o = {tag: Object.prototype.toString}; o.tag() === '[object Object]'",
        // A method call through an index passes the array as receiver.
        "let a = [function (n) { return n * 2; }]; a[0](21) === 42",
        "let o = {m: Object.prototype.valueOf}; o['m']() === o",
        "let o = {f: function () { return this; }}; o['f']() === o",
        "let a = [function () { return this; }]; a[0]() === a",
        // An element or named property shadows the chain, a hole does not.
        "let a = [1]; a.valueOf = 5; a.valueOf === 5 && [].valueOf !== 5",
        "let a = []; a[0] = 1; a.toString = 2; a.length === 1 && a.toString === 2",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
    for (src, needle) in [
        ("let a = [1];\na.toString();", "Array.prototype.toString"),
        ("let a = [];\na.constructor();", "Array constructor"),
    ] {
        let (kind, message, line, _) = failure_at(src);
        assert_eq!(kind, VmErrorKind::UnsupportedOperand, "{src}: {message}");
        assert!(message.contains(needle), "{src}: {message}");
        assert_eq!(line, 2, "{src}");
    }
    // Array.prototype is a shared built-in object: writes through
    // `constructor.prototype` are rejected like any built-in's.
    assert_eq!(
        string(
            "let r = 'none'; try { [].constructor.prototype.x = 1; } \
             catch (e) { r = 'caught'; } r"
        ),
        "caught"
    );
    assert_eq!(
        eval("[].constructor.prototype[0] === undefined && [][0] === undefined"),
        ValueView::Bool(true)
    );

    // Destructuring and spread are their own pellets.
    for src in ["[a] = [1];", "let [a] = [1];", "x = [...a];"] {
        let e = compile_script("t", src).unwrap_err();
        assert_eq!(e.kind, SyntaxErrorKind::Unsupported, "{src}: {e}");
    }

    // The literal compiles to the array instructions.
    let ops = opcodes(&exe("let a = [1, , 2]; a[0] = a[2];"));
    for op in [
        Opcode::NewArray,
        Opcode::AppendElement,
        Opcode::AppendHole,
        Opcode::GetElem,
        Opcode::PutElem,
    ] {
        assert!(ops.contains(&op), "{op:?} missing from {ops:?}");
    }

    // Deep literals and long index chains compile within the 2 MiB stack
    // the parser is tested with.
    let nested = format!("x = {}1{};", "[".repeat(62), "]".repeat(62));
    let chain = format!("let a = [];\na{};", "[0]".repeat(240));
    let handle = std::thread::Builder::new()
        .stack_size(2 << 20)
        .spawn(move || {
            let a = compile_script("nested", &nested).map(|_| ());
            let b = compile_script("chain", &chain).map(|_| ());
            (a, b)
        })
        .unwrap();
    let (a, b) = handle.join().unwrap();
    a.unwrap();
    b.unwrap();
}

/// Run sloppy (non-strict) script `src` to completion.
fn sloppy_eval(src: &str) -> ValueView {
    let exe = compile_script_with("sloppy", src, ScriptOptions::SLOPPY)
        .unwrap_or_else(|e| panic!("{src:?}: {e}"));
    match run(&exe, &[]) {
        RunOutcome::Complete(v) => v.view(),
        other => panic!("{src:?}: expected Complete, got {other:?}"),
    }
}

/// `Object.defineProperty` and `Object.getOwnPropertyDescriptor` with data
/// property attributes, and `[[Set]]` honouring `writable`
/// (clj-quickjs-119; `src/vm/descriptor.rs`). The `a.y === 3` row follows
/// `test` (line 124) of `research/quickjs/tests/test_builtin.js`, revision
/// 7d1c13c5d8f8 (`new Object()` written `Object()`, the `assert` call as a
/// Rust assertion); the QuickJS MIT notice on
/// `object_literals_and_named_properties` covers it. The rest are
/// synthetic fixtures, not light-cljs output.
#[test]
fn define_property_data_descriptors() {
    let caught = |body: &str| string(&format!("let r = 'none'; {body}"));
    // The upstream row.
    assert_eq!(
        num("let a = Object(); a.x = 1; \
             Object.defineProperty(a, 'y', { value: 3, writable: true, configurable: true, enumerable: true }); \
             a.y"),
        3.0
    );
    // defineProperty returns its target; absent attributes are false.
    assert_eq!(
        eval("let o = {}; Object.defineProperty(o, 'k', {value: 1}) === o"),
        ValueView::Bool(true)
    );
    let k = "let o = {}; Object.defineProperty(o, 'k', {value: 1}); \
             let d = Object.getOwnPropertyDescriptor(o, 'k');";
    assert_eq!(
        eval(&format!(
            "{k} d.value === 1 && d.writable === false && d.enumerable === false && d.configurable === false"
        )),
        ValueView::Bool(true)
    );
    // Assigning the non-writable `k` throws a catchable TypeError (strict
    // code) and changes nothing.
    assert_eq!(
        caught(&format!(
            "{k} try {{ o.k = 2; }} catch (e) {{ r = 'caught'; }} r + ':' + o.k"
        )),
        "caught:1"
    );
    let (kind, message, line, column) =
        failure_at("let o = {};\nObject.defineProperty(o, 'k', {value: 1});\no.k = 2;");
    assert_eq!(
        (kind, message.as_str(), line, column),
        (
            VmErrorKind::TypeError,
            "cannot assign to read-only property 'k'",
            3,
            2
        )
    );
    // Redefining the non-configurable `k` with another value throws.
    assert_eq!(
        caught(&format!(
            "{k} try {{ Object.defineProperty(o, 'k', {{value: 2}}); }} catch (e) {{ r = 'caught'; }} r + ':' + o.k"
        )),
        "caught:1"
    );
    let (kind, message, ..) =
        failure_at(&format!("{k} Object.defineProperty(o, 'k', {{value: 2}})"));
    assert_eq!(kind, VmErrorKind::TypeError, "{message}");
    assert!(message.contains("cannot define property 'k'"), "{message}");
    // A configurable property can change `writable` and `value`.
    assert_eq!(
        caught(
            "let o = {}; \
             Object.defineProperty(o, 'p', {value: 1, configurable: true}); \
             Object.defineProperty(o, 'p', {value: 2, writable: true}); \
             o.p = 3; \
             let d = Object.getOwnPropertyDescriptor(o, 'p'); \
             r = '' + o.p + d.writable + d.enumerable + d.configurable; r"
        ),
        "3truefalsetrue"
    );
    // A descriptor with both value and get throws.
    assert_eq!(
        caught(
            "let f = () => 1; let o = {}; \
             try { Object.defineProperty(o, 'x', {value: 1, get: f}); } catch (e) { r = 'caught'; } \
             r + ':' + Object.getOwnPropertyDescriptor(o, 'x')"
        ),
        "caught:undefined"
    );
    // A non-object target throws.
    assert_eq!(
        caught("try { Object.defineProperty(1, 'x', {}); } catch (e) { r = 'caught'; } r"),
        "caught"
    );
    // A missing property has no descriptor.
    assert_eq!(
        eval(
            "let o = {}; Object.defineProperty(o, 'k', {value: 1}); \
             Object.getOwnPropertyDescriptor(o, 'missing')"
        ),
        ValueView::Undefined
    );
    // Literal properties have all three attributes.
    assert_eq!(
        eval(
            "let d = Object.getOwnPropertyDescriptor({x: 1}, 'x'); \
             d.value === 1 && d.writable && d.enumerable && d.configurable"
        ),
        ValueView::Bool(true)
    );
    // The `docs/grammar.md` example lines, each true.
    for src in [
        r#"let o = {}; Object.defineProperty(o, "k", {value: 1}) === o && Object.getOwnPropertyDescriptor(o, "k").writable === false;"#,
        r#"let p = {}; Object.defineProperty(p, "k", {value: 1}); let r = false; try { p.k = 2; } catch (e) { r = p.k === 1; } r;"#,
        r#"let d = Object.getOwnPropertyDescriptor({x: 1}, "x"); d.writable && d.enumerable && d.configurable && Object.getOwnPropertyDescriptor({}, "x") === undefined;"#,
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
    // An object inheriting a non-writable `k` (`Object.create`,
    // clj-quickjs-121): strict code throws a TypeError at the write, sloppy
    // code ignores it, and the child gets no own `k` either way.
    let inherit = "let p = {}; Object.defineProperty(p, 'k', {value: 1}); \
                   let child = Object.create(p);";
    let (kind, message, line, column) = failure_at(&format!("{inherit}\nchild.k = 2;"));
    assert_eq!(kind, VmErrorKind::TypeError, "{message}");
    assert_eq!(message, "cannot assign to read-only property 'k'");
    assert_eq!((line, column), (2, 6));
    assert_eq!(
        caught(&format!(
            "{inherit} try {{ child.k = 2; }} catch (e) {{ r = 'caught'; }} \
             r + ':' + child.k + ':' + Object.getOwnPropertyDescriptor(child, 'k')"
        )),
        "caught:1:undefined"
    );
    assert_eq!(
        sloppy_eval(&format!(
            "{inherit} child.k = 2; child.k === 1 && Object.getOwnPropertyDescriptor(child, 'k') === undefined"
        )),
        ValueView::Bool(true)
    );
}

/// The rest of ValidateAndApplyPropertyDescriptor, ToPropertyDescriptor and
/// the targets `defineProperty`/`getOwnPropertyDescriptor` accept or reject
/// (clj-quickjs-119), and sloppy-mode writes of read-only properties.
/// Synthetic fixtures, not light-cljs output.
#[test]
fn define_property_validation_and_targets() {
    let fixed = "let o = {}; Object.defineProperty(o, 'k', {value: 1, writable: true});";
    let desc = |src: &str| {
        string(&format!(
            "{src} let d = Object.getOwnPropertyDescriptor(o, 'k'); \
             '' + d.value + ':' + d.writable + d.enumerable + d.configurable"
        ))
    };
    // Accepted updates: same-value rewrites of a non-configurable property,
    // writable true to false, partial updates keeping the other fields.
    for (update, expected) in [
        ("{value: 1}", "1:truefalsefalse"),
        ("{value: 2}", "2:truefalsefalse"),
        ("{writable: false}", "1:falsefalsefalse"),
        (
            "{enumerable: false, configurable: false}",
            "1:truefalsefalse",
        ),
        ("{}", "1:truefalsefalse"),
    ] {
        let src = format!("{fixed} Object.defineProperty(o, 'k', {update});");
        assert_eq!(desc(&src), expected, "{update}");
    }
    assert_eq!(
        desc(&format!(
            "{fixed} Object.defineProperty(o, 'k', {{writable: false}}); \
             Object.defineProperty(o, 'k', {{value: 1, writable: false}});"
        )),
        "1:falsefalsefalse"
    );
    // A configurable property accepts any data update, generic ones keep
    // the value; ToBoolean of the attributes.
    assert_eq!(
        desc(
            "let o = {k: 1}; Object.defineProperty(o, 'k', {enumerable: 0, writable: ''}); \
             Object.defineProperty(o, 'k', {configurable: 'yes'});"
        ),
        "1:falsefalsetrue"
    );
    // A field present with the value undefined is present.
    assert_eq!(
        desc("let o = {k: 1}; Object.defineProperty(o, 'k', {value: undefined});"),
        "undefined:truetruetrue"
    );
    assert_eq!(
        desc("let o = {}; Object.defineProperty(o, 'k', {writable: true});"),
        "undefined:truefalsefalse"
    );
    // A number key is its ToString.
    assert_eq!(
        num("let o = {}; Object.defineProperty(o, 1.5, {value: 2}); \
             Object.getOwnPropertyDescriptor(o, '1.5').value"),
        2.0
    );
    // Rejected: catchable TypeErrors that change nothing.
    for (setup, define) in [
        (fixed, "Object.defineProperty(o, 'k', {configurable: true})"),
        (fixed, "Object.defineProperty(o, 'k', {enumerable: true})"),
        (
            "let o = {}; Object.defineProperty(o, 'k', {value: 1});",
            "Object.defineProperty(o, 'k', {writable: true})",
        ),
        (
            "let o = {}; Object.defineProperty(o, 'k', {value: NaN});",
            "Object.defineProperty(o, 'k', {value: 0})",
        ),
        (
            "let o = {}; Object.defineProperty(o, 'k', {value: 0});",
            "Object.defineProperty(o, 'k', {value: -0})",
        ),
        (fixed, "Object.defineProperty(o, 'k', {get: () => 1})"),
        ("let o = {};", "Object.defineProperty(o, 'k', 1)"),
        ("let o = {};", "Object.defineProperty(o, 'k')"),
        ("let o = {};", "Object.defineProperty(o, 'k', {get: 1})"),
        ("let o = {};", "Object.defineProperty(o, 'k', {set: null})"),
        (
            "let o = {};",
            "Object.defineProperty(o, 'k', {writable: true, set: undefined})",
        ),
        ("let o = {};", "Object.defineProperty(undefined, 'k', {})"),
        ("let o = {};", "Object.defineProperty('s', 'k', {})"),
        ("let o = {};", "Object.getOwnPropertyDescriptor(null, 'k')"),
        ("let o = {};", "Object.getOwnPropertyDescriptor()"),
        // Built-in objects are immutable.
        (
            "let o = {};",
            "Object.defineProperty(Object.prototype, 'k', {value: 1})",
        ),
        (
            "let o = {};",
            "Object.defineProperty(Object, 'name', {value: 'O'})",
        ),
        // The runtime's other heap values have no properties and take none.
        (
            "let o = {};",
            "Object.defineProperty(__rt.vector(), 'k', {value: 1})",
        ),
    ] {
        let (kind, message, ..) = failure_at(&format!("{setup} {define}"));
        assert_eq!(kind, VmErrorKind::TypeError, "{define}: {message}");
        let src = format!(
            "{setup} let r = 'none'; try {{ {define}; }} catch (e) {{ r = 'caught'; }} \
             let d = Object.getOwnPropertyDescriptor(o, 'k'); \
             r + ':' + (d === undefined ? 'absent' : '' + d.value + d.writable + d.enumerable + d.configurable)"
        );
        let unchanged = string(&src);
        assert!(unchanged.starts_with("caught:"), "{define}: {unchanged}");
        if setup == fixed {
            assert_eq!(unchanged, "caught:1truefalsefalse", "{define}");
        }
    }
    assert_eq!(
        num("let f=()=>1; Object.defineProperty(f,'length',{value:1}); f.length"),
        1.0
    );
    assert_eq!(
        string("Object.getOwnPropertyDescriptor(() => 1, 'name').value"),
        ""
    );
    // What needs a later pellet fails explicitly: accessors
    // (clj-quickjs-120), wrapper objects (clj-quickjs-72), ToPrimitive of a
    // key (clj-quickjs-70).
    for (src, needle) in [
        ("Object.defineProperty({}, 'k', {get: () => 1})", "accessor"),
        (
            "Object.defineProperty({}, 'k', {set: undefined})",
            "accessor",
        ),
        (
            "let o = {k: 1}; Object.defineProperty(o, 'k', {get: undefined})",
            "accessor",
        ),
        ("Object.getOwnPropertyDescriptor(1, 'k')", "wrapper object"),
        ("Object.defineProperty({}, {}, {value: 1})", "ToPrimitive"),
    ] {
        let (kind, message, ..) = failure_at(src);
        assert_eq!(kind, VmErrorKind::UnsupportedOperand, "{src}: {message}");
        assert!(message.contains(needle), "{src}: {message}");
    }
    // Descriptors of the realm's intrinsic properties (ECMAScript §18
    // attributes) and of a value without properties.
    for (src, expected) in [
        ("let o = Object; let k = 'length';", "1:falsefalsetrue"),
        ("let o = Object; let k = 'name';", "Object:falsefalsetrue"),
        (
            "let o = Object.prototype; let k = 'constructor';",
            "true:truefalsetrue",
        ),
        (
            "let o = Object; let k = 'getOwnPropertyDescriptor';",
            "true:truefalsetrue",
        ),
        (
            "let o = Object; let k = 'prototype';",
            "true:falsefalsefalse",
        ),
    ] {
        let src = format!(
            "{src} let d = Object.getOwnPropertyDescriptor(o, k); \
             let v = typeof d.value === 'object' || typeof d.value === 'function' ? true : d.value; \
             '' + v + ':' + d.writable + d.enumerable + d.configurable"
        );
        assert_eq!(string(&src), expected, "{src}");
    }
    assert_eq!(
        eval("Object.getOwnPropertyDescriptor(__rt.vector(), 'k')"),
        ValueView::Undefined
    );
    assert_eq!(num("Object.defineProperty.length"), 3.0);
    assert_eq!(
        string("Object.getOwnPropertyDescriptor.name"),
        "getOwnPropertyDescriptor"
    );
    // Each call returns a fresh descriptor object that inherits from
    // Object.prototype and can be changed.
    assert_eq!(
        eval(
            "let o = {k: 1}; let a = Object.getOwnPropertyDescriptor(o, 'k'); \
             let b = Object.getOwnPropertyDescriptor(o, 'k'); \
             a.value = 2; a !== b && b.value === 1 && a.toString() === '[object Object]'"
        ),
        ValueView::Bool(true)
    );

    // Arrays (clj-quickjs-118): elements and `length` keep attributes too;
    // named properties are ordinary.
    for (src, expected) in [
        (
            "let a = [1, 2]; Object.defineProperty(a, 0, {value: 5}); '' + a[0] + a.length",
            "52",
        ),
        (
            "let a = [1]; Object.defineProperty(a, '3', {value: 4, writable: true, enumerable: true, configurable: true}); \
             '' + a[3] + a.length + a[2]",
            "44undefined",
        ),
        (
            "let a = [1]; Object.defineProperty(a, 'length', {value: 1, writable: true}); '' + a.length",
            "1",
        ),
        (
            "let a = [1]; Object.defineProperty(a, 'x', {value: 2}); let d = Object.getOwnPropertyDescriptor(a, 'x'); \
             '' + d.value + d.writable + a.length",
            "2false1",
        ),
        (
            "let d = Object.getOwnPropertyDescriptor([7], 0); '' + d.value + d.writable + d.enumerable + d.configurable",
            "7truetruetrue",
        ),
        (
            "let d = Object.getOwnPropertyDescriptor([7, , 9], 'length'); '' + d.value + d.writable + d.enumerable + d.configurable",
            "3truefalsefalse",
        ),
        (
            "'' + Object.getOwnPropertyDescriptor([7, , 9], 1)",
            "undefined",
        ),
    ] {
        assert_eq!(string(src), expected, "{src}");
    }
    let (kind, message, ..) =
        failure_at("Object.defineProperty([1], 'length', {configurable: true})");
    assert_eq!(kind, VmErrorKind::TypeError, "{message}");
    // ArraySetLength converts and changes the value (clj-quickjs-154).
    assert_eq!(
        num("Object.defineProperty([1], 'length', {value: 0}).length"),
        0.0
    );
    assert_eq!(
        num("Object.defineProperty([1], 'length', {value: '1'}).length"),
        1.0
    );
    // Accessors still fail explicitly.
    let src = "Object.defineProperty([1], 0, {get: () => 1})";
    let (kind, message, ..) = failure_at(src);
    assert_eq!(kind, VmErrorKind::UnsupportedOperand, "{src}: {message}");
}

/// Element and `length` attributes of arrays (clj-quickjs-119 over the
/// arrays of clj-quickjs-118), and computed-key writes of read-only
/// properties in strict and sloppy code. Synthetic fixtures, not
/// light-cljs output.
#[test]
fn define_property_array_data_attributes() {
    let desc = |target: &str, key: &str| {
        format!(
            "let d = Object.getOwnPropertyDescriptor({target}, {key}); \
             '' + d.value + ':' + d.writable + d.enumerable + d.configurable"
        )
    };
    // A new element defaults its attributes to false and extends length.
    let fresh = "let a = []; Object.defineProperty(a, 0, {value: 1});";
    assert_eq!(
        string(&format!("{fresh} {} + ':' + a.length", desc("a", "0"))),
        "1:falsefalsefalse:1"
    );
    // A read-only element rejects writes (strict), keeps its value, and
    // rejects redefinition when it is also non-configurable.
    for write in [
        "a[0] = 2",
        "a['0'] = 2",
        "Object.defineProperty(a, 0, {value: 2})",
    ] {
        let src = format!(
            "{fresh} let r = 'none'; try {{ {write}; }} catch (e) {{ r = 'caught'; }} r + a[0]"
        );
        assert_eq!(string(&src), "caught1", "{write}");
        let (kind, message, ..) = failure_at(&format!("{fresh} {write}"));
        assert_eq!(kind, VmErrorKind::TypeError, "{write}: {message}");
    }
    let (kind, message, line, column) =
        failure_at("let a = [1];\nObject.defineProperty(a, 0, {writable: false});\na[0] = 2;");
    assert_eq!(
        (kind, message.as_str(), line, column),
        (
            VmErrorKind::TypeError,
            "cannot assign to read-only property '0'",
            3,
            2
        )
    );
    // An existing element: partial updates keep the other attributes; a
    // configurable one can become writable again.
    assert_eq!(
        string(&format!(
            "let a = [1, 2]; Object.defineProperty(a, 1, {{writable: false}}); {}",
            desc("a", "1")
        )),
        "2:falsetruetrue"
    );
    assert_eq!(
        string(
            "let a = [1]; Object.defineProperty(a, 0, {writable: false}); \
             Object.defineProperty(a, 0, {writable: true}); a[0] = 3; '' + a[0]"
        ),
        "3"
    );
    // A read-only length: reported, kept, and new elements at or beyond it
    // are rejected; existing elements stay writable.
    let frozen = "let a = [1, 2]; Object.defineProperty(a, 'length', {writable: false});";
    assert_eq!(
        string(&format!("{frozen} {}", desc("a", "'length'"))),
        "2:falsefalsefalse"
    );
    for write in [
        "a[2] = 3",
        "a[5] = 3",
        "a.length = 2",
        "Object.defineProperty(a, 2, {value: 3})",
        "Object.defineProperty(a, 'length', {writable: true})",
        "Object.defineProperty(a, 'length', {value: 3})",
    ] {
        let src = format!(
            "{frozen} let r = 'none'; try {{ {write}; }} catch (e) {{ r = 'caught'; }} r + a.length + a[2]"
        );
        assert_eq!(string(&src), "caught2undefined", "{write}");
    }
    assert_eq!(
        string(&format!(
            "{frozen} a[1] = 5; Object.defineProperty(a, 'length', {{value: 2}}); '' + a[1] + a.length"
        )),
        "52"
    );

    // Computed-key writes of read-only properties: a TypeError at the `[`
    // in strict code, ignored in sloppy code (the expression is still the
    // right-hand value).
    let o = "let o = {}; Object.defineProperty(o, '0', {value: 1}); Object.defineProperty(o, 'k', {value: 1});";
    for write in ["o[0] = 2", "o['0'] = 2", "o['k'] = 2"] {
        let (kind, message, line, column) = failure_at(&format!("{o}\n{write};"));
        assert_eq!(
            (kind, line, column),
            (VmErrorKind::TypeError, 2, 2),
            "{write}: {message}"
        );
        let exe = compile_script_with(
            "sloppy",
            &format!("{o} let v = ({write}); '' + v + o[0] + o.k"),
            ScriptOptions::SLOPPY,
        )
        .unwrap();
        match run(&exe, &[]) {
            RunOutcome::Complete(v) => {
                assert_eq!(v.string().unwrap().to_string(), "211", "{write}")
            }
            other => panic!("{write}: {other:?}"),
        }
    }
    let a = "let a = [1]; Object.defineProperty(a, 0, {writable: false});";
    assert_eq!(
        sloppy_eval(&format!("{a} a[0] = 2; a[0] === 1")),
        ValueView::Bool(true)
    );
    assert_eq!(
        sloppy_eval(
            "let a = [1]; Object.defineProperty(a, 'length', {writable: false}); a[1] = 2; a.length === 1 && a[1] === undefined"
        ),
        ValueView::Bool(true)
    );
}

/// Named writes of read-only properties in sloppy code (clj-quickjs-119).
/// Synthetic fixtures, not light-cljs output.
#[test]
fn define_property_sloppy_writes() {
    // Sloppy code ignores a write that [[Set]] rejects; strict code in the
    // same script throws.
    let setup = "let o = {}; Object.defineProperty(o, 'k', {value: 1});";
    assert_eq!(
        sloppy_eval(&format!("{setup} o.k = 2; o.k")),
        ValueView::Number(1.0)
    );
    assert_eq!(
        sloppy_eval(&format!(
            "{setup} function f() {{ 'use strict'; o.k = 3; }} \
             let r = 'none'; try {{ f(); }} catch (e) {{ r = 'caught'; }} r === 'caught' && o.k === 1"
        )),
        ValueView::Bool(true)
    );
}

/// Prototype reflection (clj-quickjs-121; `src/vm/prototype.rs`):
/// `Object.getPrototypeOf`, `Object.setPrototypeOf`, `Object.create`, the
/// `__proto__` accessor and a literal's `__proto__: value`. The three
/// upstream rows follow `test` (line 124) of
/// `research/quickjs/tests/test_builtin.js`, revision 7d1c13c5d8f8 (`new
/// Object()` written `Object()`, the accessor literal `a` replaced by a data
/// literal until clj-quickjs-120, each `assert` as a Rust assertion), and
/// the `o2.__proto__ = o1` row follows `test_arrow_function` (line 128) of
/// `research/quickjs/tests/test_closure.js` (methods written as function
/// properties, the `eval("super.f()")` arrow left out); see
/// `docs/architecture.md`, "QuickJS references and provenance". The rest
/// are synthetic fixtures, not light-cljs output.
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
fn prototype_reflection() {
    let truth = |src: &str| assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    let caught = |body: &str| {
        string(&format!(
            "let r = 'none'; try {{ {body} }} catch (e) {{ r = 'caught'; }} r"
        ))
    };
    let fails = |src: &str, kind: VmErrorKind, needle: &str| {
        let (k, message, ..) = failure_at(src);
        assert_eq!(k, kind, "{src}: {message}");
        assert!(message.contains(needle), "{src}: {message}");
    };
    // The upstream rows.
    truth("let a = Object(); a.x = 1; Object.getPrototypeOf(a) === Object.prototype");
    truth("let a = {x: 1}; let b = Object.create(a); Object.getPrototypeOf(b) === a");
    truth(
        "let a = {x: 1}; let b = Object.create(a); let c = {u: 2}; \
         Object.setPrototypeOf(a, c); Object.getPrototypeOf(a) === c && b.u === 2",
    );
    truth(
        "let o1 = {f: function () { return this; }}; let o2 = {}; \
         o2.__proto__ = o1; o2.f() === o2",
    );

    // getPrototypeOf.
    truth("Object.getPrototypeOf({}) === Object.prototype");
    truth("Object.getPrototypeOf(Object.prototype) === null");
    truth(
        "Object.getPrototypeOf(Object.getOwnPropertyDescriptor({x: 1}, 'x')) === Object.prototype",
    );
    // Arrays inherit from Array.prototype (owed by clj-quickjs-118).
    truth(
        "let ap = Object.getPrototypeOf([]); ap === [].constructor.prototype && \
         ap !== Object.prototype && Object.getPrototypeOf(ap) === Object.prototype && \
         Object.getPrototypeOf([1, 2]) === ap",
    );
    // Values without JavaScript properties have a null prototype.
    truth("Object.getPrototypeOf(__rt.vector(1, 2)) === null");
    fails(
        "Object.getPrototypeOf(undefined)",
        VmErrorKind::TypeError,
        "undefined",
    );
    fails(
        "Object.getPrototypeOf(null)",
        VmErrorKind::TypeError,
        "null",
    );
    fails(
        "Object.getPrototypeOf()",
        VmErrorKind::TypeError,
        "undefined",
    );
    // Functions and built-in functions inherit from Function.prototype
    // (clj-quickjs-102), which inherits from Object.prototype.
    truth("Object.getPrototypeOf(function () {}) === Function.prototype");
    truth("Object.getPrototypeOf(() => 1) === Function.prototype");
    truth(
        "Object.getPrototypeOf(Object) === Function.prototype && \
         Object.getPrototypeOf(Function.prototype) === Object.prototype",
    );
    // A function keeps its prototype: setting the one it has is a no-op,
    // changing it is not supported yet.
    truth("let f = () => 1; Object.setPrototypeOf(f, Function.prototype) === f");
    fails(
        "Object.setPrototypeOf(() => 1, null)",
        VmErrorKind::UnsupportedOperand,
        "prototype of a function",
    );
    // Wrappers and Error.prototype do not exist yet.
    fails(
        "Object.getPrototypeOf(1)",
        VmErrorKind::UnsupportedOperand,
        "wrapper",
    );
    fails(
        "Object.getPrototypeOf('s')",
        VmErrorKind::UnsupportedOperand,
        "wrapper",
    );
    fails(
        "let e = null; try { null.x; } catch (x) { e = x; } Object.getPrototypeOf(e)",
        VmErrorKind::UnsupportedOperand,
        "Error.prototype",
    );

    // create.
    truth(
        "let o = Object.create(null); Object.getPrototypeOf(o) === null && \
         o.toString === undefined && o.x === undefined",
    );
    truth(
        "let a = {x: 1}; let b = Object.create(a); let r1 = b.x; a.x = 2; let r2 = b.x; \
         b.x = 3; r1 === 1 && r2 === 2 && b.x === 3 && a.x === 2 && \
         Object.getOwnPropertyDescriptor(b, 'x').value === 3 && \
         b.toString === Object.prototype.toString",
    );
    truth("let a = {}; Object.create(a) !== Object.create(a)");
    // A property descriptor's fields may be inherited (ToPropertyDescriptor
    // reads them with [[Get]], clj-quickjs-119).
    truth(
        "let o = {}; Object.defineProperty(o, 'k', Object.create({value: 7, writable: true})); \
         let d = Object.getOwnPropertyDescriptor(o, 'k'); d.value === 7 && d.writable && !d.enumerable",
    );
    truth(
        "typeof Object.create({}) === 'object' && Object.create.length === 2 && \
         Object.create.name === 'create'",
    );
    truth(
        "Object.getPrototypeOf.length === 1 && Object.setPrototypeOf.length === 2 && \
         Object.getPrototypeOf.name === 'getPrototypeOf' && \
         Object.setPrototypeOf.name === 'setPrototypeOf'",
    );
    for src in [
        "Object.create(1)",
        "Object.create()",
        "Object.create('p')",
        "Object.create(undefined)",
    ] {
        fails(src, VmErrorKind::TypeError, "object or null");
        assert_eq!(caught(&format!("{src};")), "caught", "{src}");
    }
    fails(
        "Object.create({}, {x: {value: 1}})",
        VmErrorKind::UnsupportedOperand,
        "defineProperties",
    );
    truth("Object.getPrototypeOf(Object.create({}, undefined)) !== null");
    // A prototype chain through a value without properties ends there.
    truth("let o = Object.create(__rt.vector(1)); o.x === undefined && o.toString === undefined");
    // A function as a prototype: its own properties (clj-quickjs-125) are
    // inherited, attributes included; names `Function.prototype` will own
    // fail explicitly (clj-quickjs-102).
    truth(
        "let f = function () {}; f.y = 2; let o = Object.create(f); \
         o.x === undefined && o.y === 2 && o.prototype === f.prototype",
    );
    truth(
        "function F() {} let o = Object.create(F); o.x = 1; o.prototype = 5; \
         o.x === 1 && o.prototype === 5 && F.prototype !== 5 && F.x === undefined && \
         Object.getOwnPropertyDescriptor(o, 'prototype').value === 5",
    );
    fails(
        "let f = function () {}; Object.defineProperty(f, 'k', {value: 1}); \
         let o = Object.create(f); o.k = 2;",
        VmErrorKind::TypeError,
        "read-only property 'k'",
    );
    fails(
        "function F() {} Object.defineProperty(F, 'prototype', {writable: false}); \
         let o = Object.create(F); o.prototype = 2;",
        VmErrorKind::TypeError,
        "read-only property 'prototype'",
    );
    truth("Object.create(function () {}).call === Function.prototype.call");
    truth("Object.create(function () {}).bind === Function.prototype.bind");
    truth("let o = Object.create(function () {}); o.bind = 1; o.bind === 1");
    // A function's `__proto__` is its [[Prototype]], Function.prototype
    // (clj-quickjs-102); changing it is not supported yet.
    truth("let f = function () {}; f.__proto__ === Function.prototype");
    truth("Object.is.__proto__ === Function.prototype");
    fails(
        "let f = function () {}; f.__proto__ = {};",
        VmErrorKind::UnsupportedOperand,
        "prototype of a function",
    );

    // setPrototypeOf.
    truth("let a = {}; let p = {y: 4}; Object.setPrototypeOf(a, p) === a && a.y === 4");
    truth(
        "let a = {x: 1}; Object.setPrototypeOf(a, null); \
         Object.getPrototypeOf(a) === null && a.toString === undefined && a.x === 1",
    );
    truth("Object.setPrototypeOf(1, null) === 1 && Object.setPrototypeOf('s', {}) === 's'");
    truth(
        "let a = [1, 2]; let p = {z: 9}; Object.setPrototypeOf(a, p); \
         a.z === 9 && a.length === 2 && a[1] === 2",
    );
    // A cycle is a TypeError and changes nothing.
    fails(
        "let a = {}; let b = Object.create(a); Object.setPrototypeOf(a, b);",
        VmErrorKind::TypeError,
        "circular",
    );
    fails(
        "let a = {}; Object.setPrototypeOf(a, a);",
        VmErrorKind::TypeError,
        "circular",
    );
    truth(
        "let a = {}; let b = Object.create(a); let c = Object.create(b); let r = 'none'; \
         try { Object.setPrototypeOf(a, c); } catch (e) { r = 'caught'; } \
         r === 'caught' && Object.getPrototypeOf(a) === Object.prototype",
    );
    // Setting the prototype an object already has is always allowed.
    truth("let a = {}; let b = Object.create(a); Object.setPrototypeOf(b, a) === b");
    truth("Object.setPrototypeOf(Object.prototype, null) === Object.prototype");
    for src in [
        "Object.setPrototypeOf(undefined, {})",
        "Object.setPrototypeOf(null, null)",
        "Object.setPrototypeOf({}, 1)",
        "Object.setPrototypeOf({}, undefined)",
        "Object.setPrototypeOf({})",
    ] {
        assert_eq!(caught(&format!("{src};")), "caught", "{src}");
        fails(src, VmErrorKind::TypeError, "");
    }
    // Built-in objects are shared and immutable, their prototypes included.
    fails(
        "Object.setPrototypeOf(Object.prototype, {})",
        VmErrorKind::TypeError,
        "built-in",
    );
    fails(
        "Object.setPrototypeOf([].constructor.prototype, null)",
        VmErrorKind::TypeError,
        "built-in",
    );
    fails(
        "Object.setPrototypeOf(Object, null)",
        VmErrorKind::TypeError,
        "built-in",
    );
    truth("Object.setPrototypeOf(Object, Function.prototype) === Object");
    // A value without JavaScript properties is not extensible.
    fails(
        "Object.setPrototypeOf(__rt.vector(1), {})",
        VmErrorKind::TypeError,
        "not extensible",
    );
    truth("let v = __rt.vector(1); Object.setPrototypeOf(v, null) === v");
    fails(
        "Object.setPrototypeOf(function () {}, null)",
        VmErrorKind::UnsupportedOperand,
        "prototype of a function",
    );

    // The `__proto__` accessor of Object.prototype.
    truth("let o = {}; o.__proto__ === Object.prototype && o['__proto__'] === Object.prototype");
    truth("[].__proto__ === Object.getPrototypeOf([])");
    truth("Object.prototype.__proto__ === null");
    truth(
        "let p = {x: 5}; let o = {}; let v = (o.__proto__ = p); \
         v === p && Object.getPrototypeOf(o) === p && o.x === 5 && o.__proto__ === p && \
         Object.getOwnPropertyDescriptor(o, '__proto__') === undefined && \
         Object.getOwnPropertyNames(o).length === 0",
    );
    truth(
        "let p = {}; let o = Object.create(p); let v = (o.__proto__ = 1); \
         v === 1 && Object.getPrototypeOf(o) === p && \
         Object.getOwnPropertyDescriptor(o, '__proto__') === undefined",
    );
    truth(
        "let o = {}; o.__proto__ = null; \
         Object.getPrototypeOf(o) === null && o.__proto__ === undefined",
    );
    truth("let o = {}; o['__proto__'] = []; o.length === 0 && Object.getPrototypeOf(o) !== null");
    // The setter's TypeErrors are thrown in sloppy code too.
    fails(
        "let o = {}; o.__proto__ = o;",
        VmErrorKind::TypeError,
        "circular",
    );
    assert_eq!(
        sloppy_eval(
            "let o = {}; let r = 'none'; try { o.__proto__ = o; } catch (e) { r = 'caught'; } \
             r === 'caught'"
        ),
        ValueView::Bool(true)
    );
    fails(
        "Object.prototype.__proto__ = {};",
        VmErrorKind::TypeError,
        "built-in",
    );
    // The setter runs before the immutability of built-in objects applies:
    // a no-op on Object.prototype is allowed in both modes.
    let no_op = "Object.prototype.__proto__ = null; Object.prototype.__proto__ = 1; \
                 [].constructor.prototype.__proto__ = Object.prototype; \
                 Object.getPrototypeOf(Object.prototype) === null";
    truth(no_op);
    assert_eq!(sloppy_eval(no_op), ValueView::Bool(true));
    // Inherited read-only array elements reject writes of their index, on
    // an array receiver and an ordinary one, without adding an element.
    let elements = "let p = []; Object.defineProperty(p, 0, {value: 1}); \
                    let a = []; Object.setPrototypeOf(a, p); let o = Object.create(p);";
    for write in ["a[0] = 2;", "o[0] = 2;", "a['0'] = 2;"] {
        fails(
            &format!("{elements} {write}"),
            VmErrorKind::TypeError,
            "read-only property '0'",
        );
    }
    assert_eq!(
        sloppy_eval(&format!(
            "{elements} a[0] = 2; o[0] = 2; a[0] === 1 && a.length === 0 && o[0] === 1 && \
             Object.getOwnPropertyDescriptor(a, 0) === undefined && \
             Object.getOwnPropertyDescriptor(o, 0) === undefined"
        )),
        ValueView::Bool(true)
    );
    truth(
        "let p = [5]; let a = []; Object.setPrototypeOf(a, p); a[0] = 6; a[0] === 6 && p[0] === 5 && a.length === 1",
    );
    // Without Object.prototype on the chain, `__proto__` is an ordinary
    // property.
    truth(
        "let o = Object.create(null); let p = {x: 1}; o.__proto__ = p; \
         Object.getPrototypeOf(o) === null && o.x === undefined && o.__proto__ === p && \
         Object.getOwnPropertyDescriptor(o, '__proto__').value === p",
    );
    // An own `__proto__` data property shadows the accessor.
    truth(
        "let o = {}; Object.defineProperty(o, '__proto__', {value: 7, writable: true}); \
         o.__proto__ = 8; o.__proto__ === 8 && Object.getPrototypeOf(o) === Object.prototype",
    );
    // The accessor is not a stored property until clj-quickjs-120.
    fails(
        "Object.getOwnPropertyDescriptor(Object.prototype, '__proto__')",
        VmErrorKind::UnsupportedOperand,
        "accessor",
    );

    // `__proto__: value` in a literal; its own names list no `__proto__`
    // (clj-quickjs-123).
    truth(
        "let p = {x: 3}; let o = {__proto__: p, y: 4}; \
         o.x === 3 && o.y === 4 && Object.getPrototypeOf(o) === p && \
         Object.getOwnPropertyNames(o).length === 1 && Object.getOwnPropertyNames(o)[0] === 'y'",
    );
    truth("let p = {x: 3}; ({\"__proto__\": p}).x === 3");
    truth("Object.getPrototypeOf({__proto__: null}) === null");
    truth("Object.getPrototypeOf({__proto__: 1}) === Object.prototype");
    truth("let o = {__proto__: undefined}; Object.getPrototypeOf(o) === Object.prototype");
    truth("let p = {}; Object.getPrototypeOf({a: 1, __proto__: p, b: 2}) === p");
    // A shorthand defines an own property, and so may follow a colon form.
    truth(
        "let __proto__ = 5; let p = {}; let o = {__proto__: p, __proto__}; \
         Object.getPrototypeOf(o) === p && \
         Object.getOwnPropertyDescriptor(o, '__proto__').value === 5 && o.__proto__ === 5",
    );
    // The values are evaluated in order, each before the next property.
    truth(
        "let log = ''; let p = {}; \
         let o = {a: (log = log + 'a'), __proto__: (log = log + 'p', p), b: (log = log + 'b')}; \
         log === 'apb' && Object.getPrototypeOf(o) === p",
    );
    for src in [
        "({__proto__: 1, __proto__: 2});",
        "({__proto__: 1, \"__proto__\": 2});",
    ] {
        let e = compile_script("t", src).unwrap_err();
        assert_eq!(e.kind, SyntaxErrorKind::Invalid, "{src}: {e}");
    }
    let ops = opcodes(&exe("let o = {__proto__: null, a: 1};"));
    assert!(ops.contains(&Opcode::SetProto), "{ops:?}");

    // The `docs/grammar.md` example lines, each true.
    for src in [
        "let a = {x: 1}; let b = Object.create(a); Object.getPrototypeOf(b) === a && b.x === 1 && Object.getPrototypeOf(a) === Object.prototype;",
        "let c = {}; let e = Object.create(c); let r1 = false; try { Object.setPrototypeOf(c, e); } catch (err) { r1 = true; } r1;",
        "let n = Object.create(null); n.toString === undefined && Object.getPrototypeOf(Object.prototype) === null;",
        "let q = {y: 2}; let s = {__proto__: q}; let t = {}; t.__proto__ = q; s.y === 2 && t.__proto__ === q && ({}).__proto__ === Object.prototype;",
    ] {
        truth(src);
    }
}

/// Object extensibility and the frozen integrity level (clj-quickjs-122;
/// `src/vm/integrity.rs`): `Object.isExtensible`, `Object.preventExtensions`,
/// `Object.freeze` and `Object.isFrozen`. The first row follows the
/// extensibility rows of `test` (line 124) of
/// `research/quickjs/tests/test_builtin.js`, revision 7d1c13c5d8f8 (`var`
/// written `let`, the asserts joined into one condition); see
/// `docs/architecture.md`, "QuickJS references and provenance". The rest
/// are synthetic fixtures, not light-cljs output.
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
fn object_extensibility() {
    let truth = |src: &str| assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    let sloppy_truth = |src: &str| assert_eq!(sloppy_eval(src), ValueView::Bool(true), "{src}");
    let fails = |src: &str, kind: VmErrorKind, needle: &str| {
        let (k, message, ..) = failure_at(src);
        assert_eq!(k, kind, "{src}: {message}");
        assert!(message.contains(needle), "{src}: {message}");
    };
    let type_error = |src: &str, needle: &str| fails(src, VmErrorKind::TypeError, needle);

    // The upstream rows.
    truth(
        "let a = {x: 1}; let before = Object.isExtensible(a); Object.preventExtensions(a); \
         let err = false; try { a.y = 2; } catch (e) { err = true; } \
         before === true && Object.isExtensible(a) === false && typeof a.y === 'undefined' && err",
    );

    // preventExtensions returns its argument; existing properties stay
    // writable and configurable.
    let prevented = "let a = {x: 1}; let same = Object.preventExtensions(a) === a;";
    truth(&format!("{prevented} same"));
    truth(&format!("{prevented} a.x = 3; a.x === 3"));
    truth(&format!(
        "{prevented} Object.defineProperty(a, 'x', {{value: 4, enumerable: false}}); \
         let d = Object.getOwnPropertyDescriptor(a, 'x'); \
         d.value === 4 && d.writable && !d.enumerable && d.configurable && !Object.isFrozen(a)"
    ));
    // Adding a property: a catchable TypeError at the member expression in
    // strict code, ignored in sloppy code; defineProperty always throws.
    let (kind, message, line, column) =
        failure_at("let a = {x: 1};\nObject.preventExtensions(a);\na.y = 2;");
    assert_eq!(
        (kind, message.as_str(), line, column),
        (
            VmErrorKind::TypeError,
            "cannot add property 'y': the object is not extensible",
            3,
            2
        )
    );
    type_error(
        &format!("{prevented} a['k'] = 1;"),
        "cannot add property 'k': the object is not extensible",
    );
    sloppy_truth(
        "var a = {x: 1}; Object.preventExtensions(a); a.y = 2; a[0] = 1; \
         a.y === undefined && a[0] === undefined && a.x === 1",
    );
    type_error(
        &format!("{prevented} Object.defineProperty(a, 'w', {{value: 1}});"),
        "cannot define property 'w': the object is not extensible",
    );
    truth(&format!(
        "{prevented} let r = false; try {{ Object.defineProperty(a, 'w', {{value: 1}}); }} \
         catch (e) {{ r = Object.getOwnPropertyDescriptor(a, 'w') === undefined; }} r"
    ));
    // A non-extensible object keeps its prototype (clj-quickjs-121):
    // setting the same one succeeds, another one is a TypeError.
    truth(&format!(
        "{prevented} Object.setPrototypeOf(a, Object.prototype) === a"
    ));
    type_error(
        &format!("{prevented} Object.setPrototypeOf(a, null);"),
        "not extensible",
    );
    truth(&format!(
        "{prevented} let r = false; try {{ Object.setPrototypeOf(a, {{}}); }} \
         catch (e) {{ r = Object.getPrototypeOf(a) === Object.prototype; }} r"
    ));
    type_error(
        &format!("{prevented} a.__proto__ = {{}};"),
        "not extensible",
    );
    // A non-extensible object still loses configurable properties
    // (clj-quickjs-124), and cannot get them back.
    truth(&format!(
        "{prevented} delete a.x && a.x === undefined \
         && Object.getOwnPropertyDescriptor(a, 'x') === undefined"
    ));
    type_error(
        &format!("{prevented} delete a.x; a.x = 1;"),
        "not extensible",
    );

    // Primitives: not extensible, frozen, returned unchanged.
    truth(
        "Object.isExtensible(1) === false && Object.preventExtensions(1) === 1 \
         && Object.isFrozen('s') && Object.freeze('s') === 's' && Object.isExtensible() === false \
         && Object.freeze(null) === null && Object.isFrozen(undefined)",
    );

    // freeze.
    let frozen = "let f = Object.freeze({x: 1});";
    type_error(
        &format!("{frozen} f.x = 2;"),
        "cannot assign to read-only property 'x'",
    );
    truth(&format!(
        "{frozen} let r = false; try {{ f.x = 2; }} catch (e) {{ r = f.x === 1; }} r"
    ));
    truth(&format!(
        "{frozen} let d = Object.getOwnPropertyDescriptor(f, 'x'); \
         Object.isFrozen(f) && !Object.isExtensible(f) && d.value === 1 && !d.writable \
         && d.enumerable && !d.configurable"
    ));
    type_error(&format!("{frozen} f.y = 2;"), "not extensible");
    type_error(
        &format!("{frozen} Object.defineProperty(f, 'x', {{value: 2}});"),
        "cannot define property 'x'",
    );
    truth(&format!(
        "{frozen} Object.defineProperty(f, 'x', {{value: 1}}) === f && Object.freeze(f) === f"
    ));
    sloppy_truth("var f = Object.freeze({x: 1}); f.x = 2; f.y = 3; f.x === 1 && f.y === undefined");
    truth("Object.isFrozen({}) === false");
    truth("Object.isFrozen(Object.preventExtensions({}))");
    truth("!Object.isFrozen(Object.preventExtensions({x: 1}))");
    // Frozen by hand: every property read-only and non-configurable.
    truth(
        "let o = {}; Object.defineProperty(o, 'k', {value: 1}); \
         let before = Object.isFrozen(o); Object.preventExtensions(o); !before && Object.isFrozen(o)",
    );
    truth(
        "let o = {}; Object.defineProperty(o, 'k', {value: 1, writable: true}); \
         Object.preventExtensions(o); !Object.isFrozen(o)",
    );
    // Freezing is shallow.
    truth("let o = Object.freeze({inner: {n: 1}}); o.inner.n = 2; o.inner.n === 2");

    // Arrays: a frozen array rejects index writes and `length` writes.
    let array = "let a = Object.freeze([1, , 3]);";
    type_error(
        &format!("{array} a[0] = 9;"),
        "cannot assign to read-only property '0'",
    );
    type_error(&format!("{array} a[1] = 9;"), "property '1'");
    type_error(&format!("{array} a[7] = 9;"), "property '7'");
    type_error(
        &format!("{array} a.length = 0;"),
        "cannot assign to read-only property 'length'",
    );
    type_error(&format!("{array} a.name = 1;"), "not extensible");
    type_error(
        &format!("{array} Object.defineProperty(a, 0, {{value: 2}});"),
        "cannot define property '0'",
    );
    truth(&format!(
        "{array} let d = Object.getOwnPropertyDescriptor(a, 0); \
         let l = Object.getOwnPropertyDescriptor(a, 'length'); \
         Object.isFrozen(a) && !Object.isExtensible(a) && a.length === 3 && a[0] === 1 \
         && a[1] === undefined && !d.writable && d.enumerable && !d.configurable && !l.writable \
         && Object.defineProperty(a, 0, {{value: 1}}) === a"
    ));
    sloppy_truth(
        "var a = Object.freeze([1, 2]); a[0] = 9; a[5] = 9; a.length = 0; \
         a[0] === 1 && a.length === 2 && a[5] === undefined",
    );
    // An element given its own attributes keeps `enumerable` when frozen.
    truth(
        "let a = [1, 2]; Object.defineProperty(a, 1, {enumerable: false}); Object.freeze(a); \
         let d = Object.getOwnPropertyDescriptor(a, 1); \
         !d.enumerable && !d.writable && !d.configurable && Object.isFrozen(a)",
    );
    // A non-extensible array keeps writable elements but takes no new ones.
    truth(
        "let a = [1]; Object.preventExtensions(a); a[0] = 5; let r = false; \
         try { a[1] = 2; } catch (e) { r = true; } \
         r && a[0] === 5 && a.length === 1 && !Object.isFrozen(a)",
    );
    // An array frozen by hand.
    truth(
        "let a = [1]; Object.defineProperty(a, 0, {writable: false, configurable: false}); \
         Object.defineProperty(a, 'length', {writable: false}); \
         let before = Object.isFrozen(a); Object.preventExtensions(a); !before && Object.isFrozen(a)",
    );
    truth("Object.isFrozen(Object.preventExtensions([])) === false");
    // Named (non-index) properties of an array freeze too, and a writable
    // one keeps a merely non-extensible array from being frozen.
    let named = "let a = [1]; a.tag = 'x'; Object.freeze(a);";
    type_error(
        &format!("{named} a.tag = 'y';"),
        "cannot assign to read-only property 'tag'",
    );
    truth(&format!(
        "{named} let d = Object.getOwnPropertyDescriptor(a, 'tag'); \
         !d.writable && d.enumerable && !d.configurable && a.tag === 'x' && Object.isFrozen(a)"
    ));
    truth(
        "let a = []; a.tag = 'x'; Object.defineProperty(a, 'length', {writable: false}); \
         Object.preventExtensions(a); !Object.isFrozen(a)",
    );
    truth(
        "let a = []; Object.defineProperty(a, 'length', {writable: false}); \
         Object.isFrozen(Object.preventExtensions(a))",
    );

    // Function objects (clj-quickjs-125): their property record holds the
    // flag and the attributes.
    truth("let g = () => 1; Object.isExtensible(g) && !Object.isFrozen(g)");
    truth(
        "let g = () => 1; g.x = 1; Object.freeze(g) === g && Object.isFrozen(g) \
         && !Object.isExtensible(g) && g() === 1 && g.x === 1",
    );
    type_error(
        "let g = () => 1; Object.freeze(g); g.x = 1;",
        "not extensible",
    );
    type_error(
        "let g = () => 1; g.x = 1; Object.freeze(g); g.x = 2;",
        "read-only property 'x'",
    );
    // A constructor's lazy `prototype` exists before the record stops
    // taking properties, and freezing makes it read-only.
    truth(
        "function F() {} Object.preventExtensions(F); let p = F.prototype; \
         typeof p === 'object' && p.constructor === F && Object.getPrototypeOf(new F()) === p",
    );
    truth(
        "function F() {} Object.freeze(F); \
         let d = Object.getOwnPropertyDescriptor(F, 'prototype'); \
         Object.isFrozen(F) && !d.writable && !d.configurable && d.value === F.prototype",
    );
    type_error(
        "function F() {} Object.freeze(F); F.prototype = 1;",
        "read-only property 'prototype'",
    );

    // Realm intrinsics are shared and immutable: they report their
    // attributes, and changing them is a TypeError.
    truth(
        "Object.isExtensible(Object.prototype) && !Object.isFrozen(Object.prototype) \
         && Object.isExtensible(Object) && !Object.isFrozen(Object.freeze)",
    );
    type_error("Object.freeze(Object.prototype);", "built-in object");
    type_error("Object.preventExtensions(Object);", "built-in object");
    type_error(
        "Object.preventExtensions([].constructor.prototype);",
        "built-in object",
    );

    // The runtime's own values have no properties and are not extensible:
    // already frozen.
    truth(
        "let v = __rt.vector(1); let m = __rt.map('k', 1); \
         !Object.isExtensible(v) && Object.isFrozen(v) && Object.freeze(v) === v \
         && Object.preventExtensions(m) === m && Object.isFrozen(m) && !Object.isExtensible(m)",
    );
    truth(
        "let b = __rt.vectorBuilder(); !Object.isExtensible(b) && Object.isFrozen(Object.freeze(b))",
    );
    // An error object's properties are clj-quickjs-230.
    fails(
        "let e = 0; try { null.x; } catch (caught) { e = caught; } Object.isFrozen(e);",
        VmErrorKind::UnsupportedOperand,
        "error object",
    );

    // The `docs/grammar.md` example lines, each true.
    for src in [
        r#"let a = {x: 1}; Object.preventExtensions(a) === a && !Object.isExtensible(a) && Object.isExtensible({});"#,
        r#"let b = {x: 1}; Object.preventExtensions(b); let r = false; try { b.y = 2; } catch (e) { r = b.y === undefined; } r;"#,
        r#"let f = Object.freeze({x: 1}); let s = false; try { f.x = 2; } catch (e) { s = f.x === 1; } s && Object.isFrozen(f) && !Object.isFrozen({});"#,
        r#"let q = Object.freeze([1, 2]); let t = false; try { q[0] = 3; } catch (e) { t = q[0] === 1; } t && Object.isFrozen(q);"#,
        r#"Object.isExtensible(1) === false && Object.preventExtensions(1) === 1 && Object.isFrozen(__rt.vector());"#,
    ] {
        truth(src);
    }
}

/// Own-property key enumeration (clj-quickjs-123; `src/vm/keys.rs`):
/// `Object.keys` and `Object.getOwnPropertyNames` list array indices in
/// ascending order, then the other string keys in creation order. The
/// first row is `test_enum` (line 174) of
/// `research/quickjs/tests/test_builtin.js`, revision 7d1c13c5d8f8 (the
/// `assert(tab, [...])` written as a Rust comparison of the joined
/// elements and the length, since `Array.prototype.toString` is
/// clj-quickjs-39), and the `[ , 1]` row follows the
/// `Object.getOwnPropertyNames(x)` assertion of `test_spread` (line 435) of
/// `research/quickjs/tests/test_language.js` without the spread; see
/// `docs/architecture.md`, "QuickJS references and provenance". The rest
/// are synthetic fixtures, not light-cljs output.
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
fn own_property_key_order() {
    let truth = |src: &str| assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    // `setup` then the key array `expr`, as "length:k0,k1,...," with a `!`
    // after any element that is not a string.
    let listed = |setup: &str, expr: &str| {
        string(&format!(
            "{setup} let k = {expr}; let s = k.length + ':'; \
             for (let i = 0; i < k.length; i++) {{ \
               s = s + k[i] + (typeof k[i] === 'string' ? ',' : '!,'); \
             }} s"
        ))
    };
    let fails = |src: &str, kind: VmErrorKind, needle: &str| {
        let (k, message, ..) = failure_at(src);
        assert_eq!(k, kind, "{src}: {message}");
        assert!(message.contains(needle), "{src}: {message}");
    };

    // The upstream rows.
    assert_eq!(
        listed(
            "var a, tab; \
             a = {x:1, \
                  \"18014398509481984\": 1, \
                  \"9007199254740992\": 1, \
                  \"9007199254740991\": 1, \
                  \"4294967296\": 1, \
                  \"4294967295\": 1, \
                  y:1, \
                  \"4294967294\": 1, \
                  \"1\": 2}; \
             tab = Object.keys(a);",
            "tab"
        ),
        "9:1,4294967294,x,18014398509481984,9007199254740992,9007199254740991,\
         4294967296,4294967295,y,"
    );
    assert_eq!(
        listed("", "Object.getOwnPropertyNames([ , 1])"),
        "2:1,length,"
    );

    // Index keys first, ascending; the rest in creation order.
    assert_eq!(
        listed("", "Object.keys({b: 1, a: 2, 1: 3, 0: 4})"),
        "4:0,1,b,a,"
    );
    assert_eq!(
        listed(
            "let o = {}; o.x = 1; o[2] = 1; o['1'] = 1; o[-1] = 1; o[1.5] = 1; o['01'] = 1; \
             o[4294967295] = 1; o[4294967294] = 1;",
            "Object.keys(o)"
        ),
        "8:1,2,4294967294,x,-1,1.5,01,4294967295,"
    );
    // Above the key-index threshold of `crate::object` too.
    let expected: String = (1..=20)
        .map(|i| format!("{i},"))
        .chain((1..=20).rev().map(|i| format!("p{i},")))
        .collect();
    assert_eq!(
        listed(
            "let o = {}; for (let i = 20; i > 0; i--) { o['p' + i] = i; o[i] = i; }",
            "Object.keys(o)"
        ),
        format!("40:{expected}")
    );
    // A redefinition keeps the first position; inherited keys are not own.
    assert_eq!(
        listed(
            "let o = {a: 1, b: 2}; o.a = 3; Object.defineProperty(o, 'a', {value: 4});",
            "Object.keys(o)"
        ),
        "2:a,b,"
    );
    assert_eq!(
        listed("let o = Object.create({a: 1}); o.b = 2;", "Object.keys(o)"),
        "1:b,"
    );

    // Non-enumerable properties are names but not keys.
    let hidden = "let o = {a: 1}; Object.defineProperty(o, 'h', {value: 2}); o.z = 3;";
    assert_eq!(listed(hidden, "Object.keys(o)"), "2:a,z,");
    assert_eq!(listed(hidden, "Object.getOwnPropertyNames(o)"), "3:a,h,z,");
    // Freezing (clj-quickjs-122) keeps `enumerable`, so the keys stay.
    assert_eq!(
        listed("", "Object.keys(Object.freeze({a: 1, 1: 2}))"),
        "2:1,a,"
    );
    assert_eq!(listed("", "Object.keys(Object.freeze([5, , 6]))"), "2:0,2,");

    // Arrays: elements (holes absent, dense then sparse), `length`, names.
    assert_eq!(listed("", "Object.keys([1, , 3])"), "2:0,2,");
    let array = "let a = [1]; a.x = 2; a[5] = 3; a[4294967294] = 4; a[3000] = 5;";
    assert_eq!(
        listed(array, "Object.getOwnPropertyNames(a)"),
        "6:0,5,3000,4294967294,length,x,"
    );
    assert_eq!(listed(array, "Object.keys(a)"), "5:0,5,3000,4294967294,x,");
    assert_eq!(
        listed(
            "let a = [1, 2]; Object.defineProperty(a, 0, {enumerable: false});",
            "Object.keys(a)"
        ),
        "1:1,"
    );
    assert_eq!(listed("", "Object.getOwnPropertyNames([])"), "1:length,");

    // Results are fresh arrays.
    truth(
        "let o = {a: 1}; let k = Object.keys(o); \
         k !== Object.keys(o) && Object.getPrototypeOf(k) === Object.getPrototypeOf([]) && \
         (k[1] = 'b', k.length === 2) && Object.keys(o).length === 1",
    );
    truth(
        "Object.keys.length === 1 && Object.keys.name === 'keys' && \
         Object.getOwnPropertyNames.length === 1 && \
         Object.getOwnPropertyNames.name === 'getOwnPropertyNames'",
    );

    // Built-in objects and functions.
    assert_eq!(listed("", "Object.keys(Object)"), "0:");
    assert_eq!(listed("", "Object.keys(Object.prototype)"), "0:");
    truth(
        "let n = Object.getOwnPropertyNames(Object); let has = false; \
         for (let i = 0; i < n.length; i++) { if (n[i] === 'keys') has = true; } \
         n[0] === 'length' && n[1] === 'name' && n[2] === 'prototype' && has",
    );
    fails(
        "Object.getOwnPropertyNames(Object.prototype)",
        VmErrorKind::UnsupportedOperand,
        "__proto__",
    );
    // Function objects: their record's enumerable properties.
    assert_eq!(
        listed(
            "let f = function () {}; f.b = 1; f.a = 2; f.prototype;",
            "Object.keys(f)"
        ),
        "2:b,a,"
    );
    assert_eq!(listed("", "Object.keys(() => 1)"), "0:");
    assert_eq!(
        listed("", "Object.getOwnPropertyNames(function () {})"),
        "3:length,name,prototype,"
    );
    // Runtime values without JavaScript properties have no keys.
    assert_eq!(listed("", "Object.keys(__rt.vector(1, 2))"), "0:");

    // ToObject.
    for call in ["Object.keys", "Object.getOwnPropertyNames"] {
        for arg in ["null", "undefined", ""] {
            fails(&format!("{call}({arg})"), VmErrorKind::TypeError, "convert");
        }
        assert_eq!(
            string(&format!(
                "let r = 'none'; try {{ {call}(null); }} catch (e) {{ r = 'caught'; }} r"
            )),
            "caught"
        );
        fails(
            &format!("{call}('ab')"),
            VmErrorKind::UnsupportedOperand,
            "wrapper",
        );
        fails(
            &format!("{call}(1)"),
            VmErrorKind::UnsupportedOperand,
            "wrapper",
        );
    }
    fails(
        "let k; try { null.x; } catch (e) { k = Object.keys(e); } k",
        VmErrorKind::UnsupportedOperand,
        "error object",
    );

    // Deleting and re-adding a key moves it to the end (clj-quickjs-124).
    assert_eq!(
        listed(
            "let o = {a: 1, b: 2}; delete o.a; o.a = 3;",
            "Object.keys(o)"
        ),
        "2:b,a,"
    );

    // The `docs/grammar.md` example lines, each true.
    for src in [
        "let k = Object.keys({b: 1, a: 2, 1: 3, 0: 4}); k.length === 4 && k[0] === \"0\" && k[1] === \"1\" && k[2] === \"b\" && k[3] === \"a\";",
        "let o = {a: 1}; Object.defineProperty(o, \"h\", {value: 2}); Object.keys(o).length === 1 && Object.getOwnPropertyNames(o)[1] === \"h\";",
        "let n = Object.getOwnPropertyNames([ , 1]); n.length === 2 && n[0] === \"1\" && n[1] === \"length\";",
    ] {
        truth(src);
    }
}

/// `delete base()[key()]` whose key callback waits on a pending cell
/// (clj-quickjs-124): the evaluated base stays on the task's operand stack
/// across the wait and a collection, each callback runs once, in order,
/// and the delete runs after resumption, removing only that property.
#[test]
fn delete_property_reference_resumes_after_need() {
    let rt = Runtime::new();
    let script = exe("let log = ''; let o = {a: 1, b: 2}; \
         let base = function () { log = log + 'b'; return o; }; \
         let key = function () { log = log + 'k'; return __rt.need(5, 3) === 7 ? 'a' : 'b'; }; \
         let r = delete base()[key()]; \
         log + ':' + r + ':' + Object.keys(o).length + ':' + o.a + ':' + o.b");
    let task = rt.spawn(&script, &[]).unwrap();
    let key = DependencyKey::new(rt.world(), DependencyId::new(5), ResultKind::new(3));
    match task.run().unwrap() {
        RunOutcome::Waiting(k) => assert_eq!(k, key),
        other => panic!("expected Waiting, got {other:?}"),
    }
    let request = task.take_producer_requests().pop().unwrap();
    rt.collect().unwrap();
    let value = rt.root(Primitive::Number(7.0));
    let wakeups = rt
        .complete(request.key(), request.producer(), Completion::Ready(&value))
        .unwrap();
    let mut runnable = wakeups.into_runnable();
    assert_eq!(runnable.len(), 1);
    match runnable.pop().unwrap().run().unwrap() {
        RunOutcome::Complete(v) => assert_eq!(
            v.string().map(|s| s.to_string()).as_deref(),
            Some("bk:true:1:undefined:2")
        ),
        other => panic!("expected Complete, got {other:?}"),
    }
}

/// The `delete` operator on property references (clj-quickjs-124;
/// `src/vm/property.rs` `delete`). The `a = {x: 1, y: 1}`, `"abc"[100]` and
/// `null.a` rows follow `test_delete` (line 222) of
/// `research/quickjs/tests/test_language.js`, revision 7d1c13c5d8f8
/// (restated as Rust assertions; `"x" in a` checked with `in`,
/// clj-quickjs-74; the caught error checked as a
/// `TypeError` failure instead of `instanceof TypeError`); the QuickJS MIT
/// notice on `object_literals_and_named_properties` covers them. The rest
/// are synthetic fixtures, not light-cljs output.
#[test]
fn delete_property_references() {
    let truth = |src: &str| assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    let sloppy_truth = |src: &str| assert_eq!(sloppy_eval(src), ValueView::Bool(true), "{src}");
    let caught = |src: &str| {
        assert_eq!(
            string(&format!(
                "let r = 'none'; try {{ {src} }} catch (e) {{ r = 'caught'; }} r"
            )),
            "caught",
            "{src}"
        );
    };
    let fails = |src: &str, kind: VmErrorKind, needle: &str| {
        let (k, message, ..) = failure_at(src);
        assert_eq!(k, kind, "{src}: {message}");
        assert!(message.contains(needle), "{src}: {message}");
    };

    // Upstream rows.
    let a = "let a = {x: 1, y: 1};";
    truth(&format!("{a} delete a.x === true"));
    truth(&format!(
        "{a} delete a.x; a.x === undefined && ('x' in a) === false && a.y === 1"
    ));
    truth("delete 'abc'[100] === true");
    caught("delete null.a;");

    // Absent properties, computed keys, parentheses, the result type.
    truth(&format!("{a} delete a.missing"));
    truth(&format!(
        "{a} delete a['y'] && a.y === undefined && a.x === 1"
    ));
    truth(&format!(
        "{a} delete (a.x) && delete ((a)['y']) && Object.keys(a).length === 0"
    ));
    truth(
        "let o = {1: 'one', 2: 'two'}; delete o[1] && delete o['2'] && Object.keys(o).length === 0",
    );
    truth(
        "let o = {true: 1, null: 2}; delete o[true] && delete o[null] && Object.keys(o).length === 0",
    );
    truth(&format!("{a} typeof delete a.x === 'boolean'"));
    // Deleting twice: the second finds nothing.
    truth(&format!("{a} delete a.x && delete a.x"));
    // Re-adding goes last (own-key order, clj-quickjs-123).
    truth(&format!(
        "{a} delete a.x; a.x = 2; let k = Object.keys(a); k[0] === 'y' && k[1] === 'x'"
    ));
    // Only own properties: an inherited one stays visible.
    truth("let p = {x: 1}; let o = Object.create(p); delete o.x && o.x === 1 && p.x === 1");
    // Objects with more than 8 properties use a key index; positions stay
    // right after a removal.
    truth(
        "let o = {a: 0, b: 1, c: 2, d: 3, e: 4, f: 5, g: 6, h: 7, i: 8, j: 9}; \
         delete o.c && o.d === 3 && o.j === 9 && o.a === 0 && Object.keys(o).length === 9 \
         && Object.keys(o)[2] === 'd' && (o.c = 10, Object.keys(o)[9] === 'c' && o.c === 10)",
    );

    // undefined/null bases: catchable TypeErrors at the `delete`, after the
    // base and the key are evaluated and before the key is converted.
    caught("let u; delete u[0];");
    caught("delete null[{}];");
    truth("let u; let n = 0; try { delete u[n++]; } catch (e) {} n === 1");
    let (kind, message, line, column) = failure_at("let u;\nlet r = delete u[0];");
    assert_eq!(
        (kind, message.as_str(), line, column),
        (
            VmErrorKind::TypeError,
            "cannot delete property '0' of undefined",
            2,
            9
        )
    );
    fails(
        "delete null.a;",
        VmErrorKind::TypeError,
        "cannot delete property 'a' of null",
    );

    // Non-configurable properties throw in strict code and stay; sloppy
    // code gets false.
    let fixed = "let o = {}; Object.defineProperty(o, 'k', {value: 1});";
    fails(
        &format!("{fixed} delete o.k;"),
        VmErrorKind::TypeError,
        "cannot delete non-configurable property 'k'",
    );
    truth(&format!(
        "{fixed} let r = false; try {{ delete o.k; }} catch (e) {{ r = o.k === 1; }} r"
    ));
    sloppy_truth(&format!("{fixed} delete o.k === false && o.k === 1"));
    truth(
        "let o = {}; Object.defineProperty(o, 'k', {value: 1, configurable: true}); delete o.k && o.k === undefined",
    );
    // Frozen objects keep everything; absent properties are still true.
    fails(
        "let f = Object.freeze({x: 1}); delete f.x;",
        VmErrorKind::TypeError,
        "non-configurable",
    );
    truth("let f = Object.freeze({x: 1}); delete f.y");
    sloppy_truth("let f = Object.freeze({x: 1}); delete f.x === false && f.x === 1");

    // Arrays: an element becomes a hole and `length` stays.
    let arr = "let arr = [1, 2];";
    truth(&format!("{arr} delete arr[0]"));
    truth(&format!(
        "{arr} delete arr[0]; arr.length === 2 && arr[0] === undefined && arr[1] === 2 \
         && Object.getOwnPropertyDescriptor(arr, '0') === undefined \
         && Object.keys(arr).length === 1 && Object.keys(arr)[0] === '1'"
    ));
    truth(&format!(
        "{arr} delete arr['1'] && arr.length === 2 && arr[1] === undefined"
    ));
    truth(&format!(
        "{arr} delete arr[5] && delete arr[-1] && arr.length === 2"
    ));
    // A hole can be filled again; a sparse element can be deleted too.
    truth(&format!(
        "{arr} delete arr[0]; arr[0] = 3; arr[0] === 3 && arr.length === 2"
    ));
    truth(
        "let s = []; s[4294967294] = 1; delete s[4294967294] && s[4294967294] === undefined && s.length === 4294967295",
    );
    // `length` is not configurable; named properties are ordinary.
    fails(
        &format!("{arr} delete arr.length;"),
        VmErrorKind::TypeError,
        "non-configurable property 'length'",
    );
    sloppy_truth(&format!(
        "{arr} delete arr.length === false && arr.length === 2"
    ));
    truth(&format!(
        "{arr} arr.name = 1; delete arr.name && arr.name === undefined"
    ));
    // Non-configurable and frozen elements stay.
    fails(
        &format!("{arr} Object.defineProperty(arr, 0, {{configurable: false}}); delete arr[0];"),
        VmErrorKind::TypeError,
        "non-configurable property '0'",
    );
    fails(
        &format!("{arr} Object.freeze(arr); delete arr[1];"),
        VmErrorKind::TypeError,
        "non-configurable",
    );
    // An element whose attributes `defineProperty` changed loses them
    // with it: added again by assignment, it is an ordinary element.
    truth(&format!(
        "{arr} Object.defineProperty(arr, 0, {{enumerable: false}}); delete arr[0]; arr[0] = 5; \
         Object.getOwnPropertyDescriptor(arr, 0).enumerable"
    ));
    // A non-extensible array loses elements and cannot get them back.
    truth(&format!(
        "{arr} Object.preventExtensions(arr); delete arr[0] && arr.length === 2"
    ));
    fails(
        &format!("{arr} Object.preventExtensions(arr); delete arr[0]; arr[0] = 1;"),
        VmErrorKind::TypeError,
        "not extensible",
    );

    // Primitive bases act as their wrapper objects.
    fails(
        "delete 'abc'[0];",
        VmErrorKind::TypeError,
        "non-configurable property '0'",
    );
    fails(
        "delete 'abc'.length;",
        VmErrorKind::TypeError,
        "non-configurable property 'length'",
    );
    truth("delete 'abc'[3] && delete 'abc'.x && delete (1).x && delete true.x");
    sloppy_truth("delete 'abc'[2] === false && delete 'abc'['length'] === false");

    // Not a reference: evaluated for its effects, then true.
    truth("delete 1");
    truth("let n = 0; delete (n = n + 1, n) && n === 1");
    truth("let n = 0; let f = function () { n = 7; return {x: 1}; }; delete f() && n === 7");
    truth("let f = function () { return delete this; }; f()");

    // Function objects (their property record, clj-quickjs-125).
    truth("let f = function () {}; delete f.x");
    truth("let f = function () {}; f.y = 1; delete f.y && f.y === undefined");
    fails(
        "let f = function () {}; delete f.prototype;",
        VmErrorKind::TypeError,
        "non-configurable property 'prototype'",
    );
    sloppy_truth(
        "let f = function () {}; delete f.prototype === false && typeof f.prototype === 'object'",
    );
    truth("let g = () => 1; delete g.prototype");
    truth(
        "let f = function () {}; delete f.length && Object.getOwnPropertyDescriptor(f,'length')===undefined",
    );

    // Built-in objects are shared and immutable: what they have cannot be
    // deleted, in either mode.
    fails(
        "delete Object.prototype.toString;",
        VmErrorKind::TypeError,
        "built-in",
    );
    fails("delete Object.keys;", VmErrorKind::TypeError, "built-in");
    fails(
        "delete Object.prototype.__proto__;",
        VmErrorKind::TypeError,
        "built-in",
    );
    truth("delete Object.prototype.nothing && delete Object.nothing");
    fails(
        "let r = (function () { return delete Object.keys; })();",
        VmErrorKind::TypeError,
        "built-in",
    );
    // An own `__proto__` is not the accessor.
    truth(
        "let o = {}; Object.defineProperty(o, '__proto__', {value: 1, configurable: true}); delete o.__proto__ && Object.getPrototypeOf(o) === Object.prototype",
    );
    // Values without JavaScript properties have nothing to delete.
    truth("delete __rt.vector(1, 2)[0]");
    fails(
        "let r; try { null.x; } catch (e) { r = delete e.message; } r",
        VmErrorKind::UnsupportedOperand,
        "error object",
    );

    // `delete x` is a compile-time SyntaxError in strict code.
    for src in ["function f() { delete x; }", "let x = 1; delete x;"] {
        let e = compile_script("t", src).unwrap_err();
        assert_eq!(e.kind, SyntaxErrorKind::Invalid, "{src}: {e}");
    }
    let e = compile_script_with("t", "delete x;", ScriptOptions::SLOPPY).unwrap_err();
    assert_eq!(e.kind, SyntaxErrorKind::Unsupported, "{e}");

    // Run-time failures are reported at the `delete`, also when the
    // expression is parenthesized or spans lines.
    for (src, line, column) in [
        ("let r = (delete null.a);", 1, 10),
        (
            "let o = {x: 1};\nObject.freeze(o);\nlet r = (\n  delete\n  o[\n'x'] || delete o.y);",
            4,
            3,
        ),
        ("let r = ((delete (Object.keys)));", 1, 11),
    ] {
        let (kind, message, l, c) = failure_at(src);
        assert_eq!(kind, VmErrorKind::TypeError, "{src}: {message}");
        assert_eq!((l, c), (line, column), "{src}: {message}");
    }
    let (_, message, l, c) =
        failure_at("let o = {};\nObject.defineProperty(o, 'k', {value: 1});\n(delete o.k);");
    assert_eq!(
        (message.as_str(), l, c),
        ("cannot delete non-configurable property 'k'", 3, 2)
    );

    // The `docs/grammar.md` example lines, each true.
    for src in [
        "let a = {x: 1, y: 1}; delete a.x && a.x === undefined && delete a.missing && delete a[\"y\"];",
        "let arr = [1, 2]; delete arr[0] && arr.length === 2 && arr[0] === undefined;",
        "let r = false; try { delete null.a; } catch (e) { r = true; } r;",
        "let o = {}; Object.defineProperty(o, \"k\", {value: 1}); let r = false; try { delete o.k; } catch (e) { r = o.k === 1; } r;",
    ] {
        truth(src);
    }
}

/// Property reads and method calls on number and string primitives
/// through `Number.prototype` and `String.prototype`, without wrapper
/// objects (clj-quickjs-73; `docs/architecture.md`, "Primitive bases").
/// The `(19686109595169230000).toString()` row is the `test_cvt` (line 108)
/// case of `research/quickjs/tests/test_language.js`, revision
/// 7d1c13c5d8f8, restated as a Rust assertion; the QuickJS MIT notice on
/// `addition_concatenates_strings` covers it. The rest are synthetic
/// fixtures, not light-cljs output.
#[test]
fn methods_on_primitive_bases() {
    // The pellet's acceptance rows.
    for (src, want) in [
        ("(19686109595169230000).toString()", "19686109595169230000"),
        ("(1.5).toString()", "1.5"),
        ("(-0).toString()", "0"),
        ("(1e21).toString()", "1e+21"),
        ("\"abc\".toString()", "abc"),
    ] {
        assert_eq!(string(src), want, "{src}");
    }
    let truth = |src: &str| assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    truth("(1).toString === Number.prototype.toString");
    assert_eq!(eval("(1).valueOf()"), ValueView::Number(1.0));
    for src in ["let u; u.x", "null.toString()", "(1).x = 2"] {
        assert_eq!(failure(src).kind(), VmErrorKind::TypeError, "{src}");
        let caught = format!("let r = false; try {{ {src}; }} catch (e) {{ r = true; }} r");
        truth(&caught);
    }

    // More conversions, bindings, computed keys and number-literal members.
    for (src, want) in [
        ("let n = 255; n.toString()", "255"),
        ("(0 / 0).toString() + (-1 / 0).toString()", "NaN-Infinity"),
        ("(123e-20).toString()", "1.23e-18"),
        ("1..toString() + 2 .toString()", "12"),
        ("(1).toString(10) + (2).toString(undefined)", "12"),
        ("(7)['toString']()", "7"),
        ("let s = 'x'; s['valueOf']() + s.toString()", "xx"),
        ("''.toString()", ""),
        ("`a${1}`.toString()", "a1"),
        // `Number.prototype` and `String.prototype` hold +0 and "".
        ("Number.prototype.toString()", "0"),
        (
            "String.prototype.toString() + String.prototype.valueOf()",
            "",
        ),
        (
            "(1).toString.name + (1).valueOf.name + 'a'.toString.name",
            "toStringvalueOftoString",
        ),
    ] {
        assert_eq!(string(src), want, "{src}");
    }
    for src in [
        "(1).valueOf() === 1 && (2.5).valueOf() + 1 === 3.5",
        "Number.prototype.valueOf() === 0 && Object.is(Number.prototype.valueOf(), 0)",
        "Object.is((-0).valueOf(), -0)",
        "'ab'.valueOf() === 'ab'",
        "'a'.toString === String.prototype.toString && 'a'.valueOf === String.prototype.valueOf",
        "(1).toString !== 'a'.toString && (1).valueOf !== Object.prototype.valueOf",
        // The prototypes inherit from Object.prototype; their constructors
        // are the `Number` and `String` globals.
        "(1).constructor === Number && 'a'.constructor === String",
        "Object.getPrototypeOf(Number.prototype) === Object.prototype",
        "Object.getPrototypeOf(String.prototype) === Object.prototype",
        "(1).isPrototypeOf === undefined && 'toString' in Number.prototype",
        "typeof Number === 'function' && typeof String === 'function'",
        "Number.prototype.toString.length === 1 && Number.prototype.valueOf.length === 0",
        "String.prototype.toString.length === 0 && Number.length === 1 && String.name === 'String'",
        // `__proto__` reads the wrapper's prototype.
        "(1).__proto__ === Number.prototype && 'a'.__proto__ === String.prototype",
        // Absent names read undefined, also a string index past the end.
        "(1).x === undefined && 'abc'.x === undefined && 'abc'[3] === undefined",
        "'abc'[-1] === undefined && 'abc'['1.5'] === undefined",
        // `String.prototype` is the String object for "": its own `length`
        // is 0, read-only, not enumerable, not configurable.
        "String.prototype.length === 0 && 'length' in String.prototype && !('length' in Number.prototype)",
        "let d = Object.getOwnPropertyDescriptor(String.prototype, 'length'); \
         d.value === 0 && !d.writable && !d.enumerable && !d.configurable",
        "let r = false; try { String.prototype.length = 1; } catch (e) { r = true; } r",
        // The same function on every read.
        "let f = (1).toString; f === (2).toString && f === Number.prototype.toString",
    ] {
        truth(src);
    }

    // A method that needs a number or string receiver throws a catchable
    // TypeError for any other: `this` is `undefined` in a plain call.
    for (src, message) in [
        (
            "let f = (1).toString;\nf();",
            "Number.prototype.toString requires that 'this' be a Number, not undefined",
        ),
        (
            "let o = {m: (1).valueOf};\no.m();",
            "Number.prototype.valueOf requires that 'this' be a Number, not an object",
        ),
        (
            "let o = {m: 'a'.toString};\no.m();",
            "String.prototype.toString requires that 'this' be a String, not an object",
        ),
    ] {
        let (kind, m, line, _) = failure_at(src);
        assert_eq!(
            (kind, m.as_str(), line),
            (VmErrorKind::TypeError, message, 2),
            "{src}"
        );
    }
    truth("let o = {m: 'a'.valueOf}; let r = false; try { o.m(); } catch (e) { r = true; } r");

    // `undefined` and `null` bases are TypeErrors at the member operator.
    for (src, message, column) in [
        ("let u;\nu.x;", "cannot read property 'x' of undefined", 2),
        (
            "let u;\n(null).toString();",
            "cannot read property 'toString' of null",
            7,
        ),
        (
            "let u;\nu['x'] = 1;",
            "cannot set property 'x' of undefined",
            2,
        ),
    ] {
        let (kind, m, line, col) = failure_at(src);
        assert_eq!(
            (kind, m.as_str(), line, col),
            (VmErrorKind::TypeError, message, 2, column),
            "{src}"
        );
    }

    // Writes: strict code throws, since a primitive takes no property (a
    // string's index and `length` are read-only own properties of its
    // wrapper); `__proto__` runs the setter, which ignores a primitive.
    for (src, message) in [
        (
            "let n = 1;\nn.x = 2;",
            "cannot create property 'x' on a number: primitive values have no own properties",
        ),
        (
            "let s = 'abc';\ns[5] = 2;",
            "cannot create property '5' on a string: primitive values have no own properties",
        ),
        (
            "let s = 'abc';\ns[0] = 'x';",
            "cannot assign to read-only property '0' of a string",
        ),
        (
            "let s = 'abc';\ns.length = 1;",
            "cannot assign to read-only property 'length' of a string",
        ),
        (
            "let s = 'abc';\ns.toString = 1;",
            "cannot create property 'toString' on a string: primitive values have no own properties",
        ),
        (
            "let b = true;\nb.x = 1;",
            "cannot create property 'x' on a boolean: primitive values have no own properties",
        ),
        (
            "let n = 1;\nn.x += 1;",
            "cannot create property 'x' on a number: primitive values have no own properties",
        ),
    ] {
        let (kind, m, line, _) = failure_at(src);
        assert_eq!(
            (kind, m.as_str(), line),
            (VmErrorKind::TypeError, message, 2),
            "{src}"
        );
    }
    truth("let n = 1; n.__proto__ = null; 'a'.__proto__ = {}; n.__proto__ === Number.prototype");
    // Sloppy code ignores the writes.
    let sloppy = compile_script_with(
        "t",
        "let n = 1; n.x = 2; 'abc'[0] = 'y'; 'abc'.length = 0; true.x = 1; n.x === undefined",
        ScriptOptions::SLOPPY,
    )
    .unwrap();
    match run(&sloppy, &[]) {
        RunOutcome::Complete(v) => assert_eq!(v.view(), ValueView::Bool(true)),
        other => panic!("expected Complete, got {other:?}"),
    }

    // Explicitly unsupported until their pellets: a string's own `length`
    // and indices (clj-quickjs-142), boolean properties (clj-quickjs-39),
    // radixes other than 10 (clj-quickjs-39).
    // Not catchable: no JavaScript sees a wrong answer.
    for (src, needle) in [
        (
            "let s = 'abc';\ns.length;",
            "string length and index properties",
        ),
        (
            "let s = 'abc';\ns[0];",
            "string length and index properties",
        ),
        (
            "let s = 'abc';\ns['2'];",
            "string length and index properties",
        ),
        ("let b = true;\nb.toString();", "of a boolean"),
        ("let n = 1;\nn.toString(2);", "radix other than 10"),
        ("let n = 1;\nn.toString('10');", "radix other than 10"),
    ] {
        let (kind, message, line, _) = failure_at(src);
        assert_eq!(kind, VmErrorKind::UnsupportedOperand, "{src}: {message}");
        assert!(message.contains(needle), "{src}: {message}");
        assert_eq!(line, 2, "{src}");
    }
    let (kind, ..) = failure_at("let r; try { 'abc'.length; } catch (e) { r = 1; } r");
    assert_eq!(kind, VmErrorKind::UnsupportedOperand);

    // The `docs/grammar.md` example lines, each true.
    for src in [
        "(19686109595169230000).toString() === \"19686109595169230000\" && (1.5).toString() === \"1.5\" && (1).valueOf() === 1;",
        "\"abc\".toString() === \"abc\" && (1).toString === Number.prototype.toString && (1).constructor === Number;",
        "let r = 0; try { null.toString(); } catch (e) { r = 1; } try { (1).x = 2; } catch (e) { r = r + 10; } r === 11;",
    ] {
        truth(src);
    }
}

/// A method call on a primitive allocates no wrapper object
/// (clj-quickjs-73): once the realm exists, a loop of property reads and
/// method calls on numbers and strings leaves the live-object count
/// unchanged. Synthetic fixture, not light-cljs output.
#[test]
fn methods_on_primitive_bases_allocate_no_wrappers() {
    let rt = Runtime::new();
    let warm = exe("Number.prototype.toString(); 0");
    assert!(matches!(
        rt.spawn(&warm, &[]).unwrap().run().unwrap(),
        RunOutcome::Complete(_)
    ));
    rt.collect().unwrap();
    let before = rt.heap_stats().objects;
    let src = "let r = 0; for (let i = 0; i < 200; i++) { r = r + i.valueOf(); \
               if (('' + i).toString() === i.toString()) { r = r + 1; } } r";
    match rt.spawn(&exe(src), &[]).unwrap().run().unwrap() {
        RunOutcome::Complete(v) => assert_eq!(v.view(), ValueView::Number(19900.0 + 200.0)),
        other => panic!("expected Complete, got {other:?}"),
    }
    assert_eq!(rt.heap_stats().objects, before);
}

/// ArraySetLength (clj-quickjs-154). Synthetic fixtures, not light-cljs output.
/// The first four groups adapt QuickJS tests/test_builtin.js, test_array,
/// revision 7d1c13c5d8f8. Copyright (c) 2017-2021 Fabrice Bellard.
/// Copyright (c) 2017-2021 Charlie Gordon.
/// Permission is hereby granted, free of charge, to any person obtaining a copy
/// of this software and associated documentation files (the "Software"), to deal
/// in the Software without restriction, including without limitation the rights
/// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
/// copies of the Software, and to permit persons to whom the Software is
/// furnished to do so, subject to the following conditions:
/// The above copyright notice and this permission notice shall be included in
/// all copies or substantial portions of the Software.
/// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
/// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
/// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL
/// THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
/// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
/// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
/// THE SOFTWARE.
#[test]
fn array_length_assignment() {
    for src in [
        "let a = [1,2,3]; a.length = 2; a.length === 2 && a[0] === 1 && a[1] === 2 && a[2] === undefined",
        "let a = []; a[1] = 10; a[4] = 3; a.length === 5 && a[1] === 10 && a[4] === 3",
        "let a = [1,2]; a.length = 5; a[4] = 1; a.length = 4; a[4] !== 1 && a.length === 4 && a[2] === undefined",
        "let a = [1,2,3,4,5]; Object.defineProperty(a, '3', {configurable: false}); let caught = false; try { a.length = 2; } catch (e) { caught = true; } caught && a.length === 4 && a[0] === 1 && a[1] === 2 && a[2] === 3 && a[3] === 4 && a[4] === undefined",
        "let a = [1,2,3]; a.length = 0; a.length === 0 && a[0] === undefined",
        "let a = [1,2,3]; let v = (a['length'] = '2'); a.length === 2 && v === '2' && a[2] === undefined",
        "let a = []; a[4294967294] = 1; a.length = 0; a.length === 0 && a[4294967294] === undefined",
        "let a = []; a[4000] = 4; a[5000] = 5; a[6000] = 6; a.length = 5000; a.length === 5000 && a[4000] === 4 && Object.getOwnPropertyDescriptor(a, 5000) === undefined && Object.getOwnPropertyDescriptor(a, 6000) === undefined",
        "let a = [1]; a.length = 4294967295; a.length === 4294967295 && a[4294967294] === undefined && Object.keys(a).length === 1",
        "let a = [1,2]; a.length = -0; a.length === 0 && 1/a.length === 1/0",
        "let a = [1,2]; a.length = true; let n = a.length; a.length = null; n === 1 && a.length === 0 && a[0] === undefined",
        "let a = [1,2]; Object.preventExtensions(a); a.length = 5; a.length = 1; a.length === 1 && a[0] === 1 && a[1] === undefined",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
    for invalid in [
        "-1",
        "1.5",
        "4294967296",
        "0/0",
        "1/0",
        "undefined",
        "'bad'",
    ] {
        let src = format!(
            "let a = [1,2,3]; let caught = false; try {{ a.length = {invalid}; }} catch (e) {{ caught = true; }} caught && a.length === 3 && a[2] === 3"
        );
        assert_eq!(eval(&src), ValueView::Bool(true), "{src}");
    }
    let readonly = "let a = [1,2]; Object.defineProperty(a, 'length', {writable: false});";
    for write in [
        "a.length = 5",
        "a.length = 0",
        "a.length = 2",
        "a[a.length] = 1",
    ] {
        let src = format!(
            "{readonly} let caught = false; try {{ {write}; }} catch (e) {{ caught = true; }} caught && a.length === 2 && a[0] === 1 && a[2] === undefined"
        );
        assert_eq!(eval(&src), ValueView::Bool(true), "{src}");
        assert_eq!(
            sloppy_eval(&format!(
                "{readonly} {write}; a.length === 2 && a[0] === 1 && a[2] === undefined"
            )),
            ValueView::Bool(true)
        );
    }
    assert_eq!(
        sloppy_eval(
            "let a = [1,2,3,4,5]; Object.defineProperty(a, 3, {configurable:false}); let result = (a.length = 1); result === 1 && a.length === 4 && a[4] === undefined && a[3] === 4"
        ),
        ValueView::Bool(true)
    );
    for (src, kind) in [
        ("let a = [1]; a.length = -1", VmErrorKind::RangeError),
        (
            "let a = [1]; Object.defineProperty(a, 'length', {value:1.5})",
            VmErrorKind::RangeError,
        ),
        (
            "let a = [1]; Object.defineProperty(a, 'length', {writable:false}); a.length = 5",
            VmErrorKind::TypeError,
        ),
        (
            "let a = [1]; Object.defineProperty(a, 'length', {writable:false}); a[a.length] = 1",
            VmErrorKind::TypeError,
        ),
        (
            "let a = [1]; Object.defineProperty(a, 0, {configurable:false}); a.length = 0",
            VmErrorKind::TypeError,
        ),
    ] {
        assert_eq!(failure(src).kind(), kind, "{src}");
        let caught = format!("function f() {{ try {{ {src}; }} catch(e) {{ return e; }} }} f()");
        let RunOutcome::Complete(error) = run(&exe(&caught), &[]) else {
            panic!("failed to catch {src}");
        };
        assert_eq!(error.error().unwrap().kind(), kind, "{src}");
    }
    assert_eq!(
        sloppy_eval(
            "let a = [1,2]; function f() { 'use strict'; a.length = 1; a.length = -1; } let caught = false; try { f(); } catch(e) { caught = true; } caught && a.length === 1"
        ),
        ValueView::Bool(true)
    );
    assert_eq!(
        failure("let a = [1]; a.length = {}").kind(),
        VmErrorKind::UnsupportedOperand
    );
}

#[test]
fn array_length_define_property() {
    for src in [
        "let a = [1,2,3]; Object.defineProperty(a, 'length', {value:'2'}); a.length === 2 && a[2] === undefined",
        "let a = []; a[4000] = 4; a[5000] = 5; a[6000] = 6; Object.defineProperty(a, 'length', {value:5000}); a.length === 5000 && a[4000] === 4 && Object.getOwnPropertyDescriptor(a, 5000) === undefined && Object.getOwnPropertyDescriptor(a, 6000) === undefined",
        "let a = [1]; Object.defineProperty(a, 'length', {value:5, writable:false}); let d = Object.getOwnPropertyDescriptor(a, 'length'); d.value === 5 && !d.writable && !d.enumerable && !d.configurable && a[4] === undefined",
        "let a = [1,2,3,4,5]; Object.defineProperty(a, 3, {configurable:false}); let caught = false; try { Object.defineProperty(a, 'length', {value:1, writable:false}); } catch(e) { caught = true; } let d = Object.getOwnPropertyDescriptor(a, 'length'); caught && a.length === 4 && a[4] === undefined && a[3] === 4 && !d.writable",
        "let a = []; a[4294967294] = 1; Object.defineProperty(a, 4000, {value:2, configurable:false}); a[5000] = 3; let caught = false; try { Object.defineProperty(a, 'length', {value:0}); } catch(e) { caught = true; } caught && a.length === 4001 && a[4000] === 2 && a[5000] === undefined && a[4294967294] === undefined && Object.getOwnPropertyDescriptor(a, 'length').writable",
        "let a = [1]; Object.defineProperty(a, 'length', {writable:false}); Object.defineProperty(a, 'length', {value:'1'}); a.length === 1",
        "let a = [1]; let caught = false; try { Object.defineProperty(a, 'length', {value:0, enumerable:true}); } catch(e) { caught = true; } caught && a.length === 1 && a[0] === 1",
        "let a = [1,2,3,4,5]; Object.defineProperty(a, 1, {configurable:false}); Object.defineProperty(a, 3, {configurable:false}); let caught = false; try { a.length = 0; } catch(e) { caught = true; } caught && a.length === 4 && a[2] === 3 && a[4] === undefined",
        "let a = [1,2]; Object.defineProperty(a, 1, {writable:false, enumerable:false, configurable:true}); a.length = 1; a[1] = 3; let d = Object.getOwnPropertyDescriptor(a, 1); a.length === 2 && d.value === 3 && d.writable && d.enumerable && d.configurable",
        "let a = []; Object.defineProperty(a, 5000, {value:1, writable:false, configurable:true}); Object.defineProperty(a, 'length', {value:0}); a[5000] = 3; Object.getOwnPropertyDescriptor(a, 5000).writable && a[5000] === 3",
        "let a = [1,2]; Object.setPrototypeOf(a, {1:7}); a.length = 1; a.length = 2; a[1] === 7 && Object.getOwnPropertyDescriptor(a, 1) === undefined",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
    for invalid in ["-1", "1.5", "4294967296", "0/0", "undefined"] {
        let src = format!(
            "let a = [1,2]; let caught = false; try {{ Object.defineProperty(a, 'length', {{value:{invalid}, writable:false}}); }} catch(e) {{ caught = true; }} caught && a.length === 2 && Object.getOwnPropertyDescriptor(a, 'length').writable"
        );
        assert_eq!(eval(&src), ValueView::Bool(true), "{src}");
    }
}

/// Array push semantic cases; synthetic fixtures, not light-cljs output.
/// The first case adapts QuickJS tests/test_builtin.js, test_array,
/// revision 7d1c13c5d8f8. Copyright (c) 2017-2021 Fabrice Bellard.
/// Copyright (c) 2017-2021 Charlie Gordon.
/// Permission is hereby granted, free of charge, to any person obtaining a copy
/// of this software and associated documentation files (the "Software"), to deal
/// in the Software without restriction, including without limitation the rights
/// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
/// copies of the Software, and to permit persons to whom the Software is
/// furnished to do so, subject to the following conditions:
/// The above copyright notice and this permission notice shall be included in
/// all copies or substantial portions of the Software.
/// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
/// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
/// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL
/// THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
/// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
/// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
/// THE SOFTWARE.
#[test]
fn array_push() {
    for src in [
        "let a = [1, 2]; let n = a.push(3, 4); n === 4 && a[2] === 3 && a[3] === 4 && a.length === 4",
        "[].push() === 0 && Array.prototype.push.length === 1 && Array.prototype.push.name === 'push'",
        "let a = [,,]; a.push(undefined, {x: 7}); a.length === 4 && !(0 in a) && (2 in a) && a[3].x === 7",
        "let o = {length: 1}; Array.prototype.push.call(o, 'x') === 2 && o[1] === 'x' && o.length === 2",
        "let o = {}; Array.prototype.push.call(o, 7) === 1 && o[0] === 7 && o.length === 1",
        "let o = {length: '1.9'}; Array.prototype.push.call(o, 7) === 2 && o[1] === 7 && o.length === 2",
        "let o = {length: -2}; Array.prototype.push.call(o, 7) === 1 && o[0] === 7",
        "let o = {length: 0/0}; Array.prototype.push.call(o) === 0 && o.length === 0",
        "let o = {length: 1/0}; Array.prototype.push.call(o) === 9007199254740991 && o.length === 9007199254740991",
        "let o = {length: 9007199254740990}; Array.prototype.push.call(o, 7) === 9007199254740991 && o['9007199254740990'] === 7",
        "let o = {length: 9007199254740991}; let caught = false; try { Array.prototype.push.call(o, 7); } catch(e) { caught = true; } caught && o.length === 9007199254740991 && o['9007199254740991'] === undefined",
        "let p = {length: 2}; let o = Object.create(p); Array.prototype.push.call(o, 7) === 3 && o[2] === 7 && p.length === 2",
        "let a = []; a[5000] = 1; a.push(7) === 5002 && a[5001] === 7 && a[0] === undefined",
        "let a = []; a.length = 4294967295; let caught = false; try { a.push(7); } catch (e) { caught = true; } caught && a.length === 4294967295 && a['4294967295'] === 7",
        "let o = {length: 0}; Object.defineProperty(o, 1, {value: 9}); let caught = false; try { Array.prototype.push.call(o, 7, 8); } catch (e) { caught = true; } caught && o[0] === 7 && o[1] === 9 && o.length === 0",
        "let o = {}; Object.defineProperty(o, 'length', {value: 0}); let caught = false; try { Array.prototype.push.call(o, 7); } catch (e) { caught = true; } caught && o[0] === 7 && o.length === 0",
        "let a = []; Object.preventExtensions(a); a.push() === 0",
        "Array === [].constructor && Array.prototype === Object.getPrototypeOf([]) && typeof Array === 'function'",
        "let d = Object.getOwnPropertyDescriptor(Array.prototype, 'push'); d.writable && !d.enumerable && d.configurable",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
    for src in [
        "Array.prototype.push.call(null)",
        "Array.prototype.push.call(undefined)",
        "const push = [].push; push()",
        "let a = [1, 2]; Object.defineProperty(a, 'length', {writable: false}); a.push(5)",
        "let a = []; Object.defineProperty(a, 'length', {writable: false}); a.push()",
        "let a = Object.freeze([1]); a.push(5)",
        "let a = Object.freeze([]); a.push()",
        "let a = Object.preventExtensions([]); a.push(5)",
        "Array.prototype.push.call({length: 9007199254740991}, 7)",
        "Array.prototype.push.call({length: 1/0}, 7)",
        "let p = {}; Object.defineProperty(p, 0, {value: 9}); let a = []; Object.setPrototypeOf(a, p); Array.prototype.push.call(a, 7)",
        "let p = {}; Object.defineProperty(p, 0, {value: 9}); let o = Object.create(p); Array.prototype.push.call(o, 7)",
    ] {
        assert_eq!(failure(src).kind(), VmErrorKind::TypeError, "{src}");
        let caught = format!("function f() {{ try {{ {src}; }} catch(e) {{ return e; }} }} f()");
        let RunOutcome::Complete(error) = run(&exe(&caught), &[]) else {
            panic!("failed to catch {src}");
        };
        assert_eq!(
            error.error().unwrap().kind(),
            VmErrorKind::TypeError,
            "{src}"
        );
        assert_eq!(
            sloppy_eval(&format!(
                "let caught = false; try {{ {src}; }} catch(e) {{ caught = true; }} caught"
            )),
            ValueView::Bool(true),
            "{src}"
        );
    }
    for src in [
        "Array.prototype.push.call({length: {valueOf: function () { return 1; }}}, 7)",
        "Array.prototype.push.call(1, 7)",
        "Array.prototype.push.call('abc', 7)",
        "Array.prototype.push.call(true, 7)",
    ] {
        assert_eq!(
            failure(src).kind(),
            VmErrorKind::UnsupportedOperand,
            "{src}"
        );
    }

    assert_eq!(
        failure("let a = []; a.length = 4294967295; a.push(7)").kind(),
        VmErrorKind::RangeError
    );
    assert_eq!(failure("Array()").kind(), VmErrorKind::UnsupportedOperand);

    // Yield and collect throughout repeated appends, including heap-valued
    // elements whose only references are in the growing dense array.
    let rt = Runtime::new();
    let code = exe(
        "let b = []; let objects = []; let i = 0; while (i < 10000) { b.push(i); objects.push({n: i}); i = i + 1; } b.length === 10000 && b[9999] === 9999 && objects[0].n === 0 && objects[9999].n === 9999",
    );
    let task = rt.spawn(&code, &[]).unwrap();
    let mut yields = 0;
    for _ in 0..10000 {
        match task
            .run_with(Budget::instructions(1000), &mut NoEffects)
            .unwrap()
        {
            RunOutcome::Yielded => {
                yields += 1;
                rt.collect().unwrap();
            }
            RunOutcome::Complete(result) => {
                assert_eq!(result.view(), ValueView::Bool(true));
                assert!(yields > 100);
                return;
            }
            other => panic!("unexpected {other:?}"),
        }
    }
    panic!("push loop did not finish within bounded slices");
}

#[test]
fn array_push_rejects_foreign_arrays() {
    let rt = Runtime::new();
    let producer = rt.spawn(&exe("[]"), &[]).unwrap();
    let RunOutcome::Complete(array) = producer.run().unwrap() else {
        panic!("array producer failed");
    };
    for src in [
        "function f(a) { return a.push(1); }",
        "function f(a) { return Array.prototype.push.call(a, 1); }",
    ] {
        let code = compile_function(src).unwrap();
        let consumer = rt.spawn(&code, &[&array]).unwrap();
        let RunOutcome::Failed(error) = consumer.run().unwrap() else {
            panic!("foreign array was accepted");
        };
        assert_eq!(
            error.kind(),
            VmErrorKind::ForeignObject {
                owner: Some(producer.id()),
                caller: consumer.id()
            }
        );
    }
}

/// Synthetic fixtures, not light-cljs output: OrdinaryToPrimitive calls
/// methods through the VM with their object as receiver.
#[test]
fn objects_convert_with_to_primitive() {
    for src in [
        "({}) != 'abc'",
        "({}) == '[object Object]'",
        "({valueOf: function () { return 1; }}) == 1",
        "1 < ({valueOf: function () { return 2; }})",
        "let o = {}; o == o",
        "let o = {valueOf: function () { throw 1; }}; o != null && o != undefined && o != {}",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
    assert_eq!(string("'' + {}"), "[object Object]");
    assert_eq!(num("({valueOf: function () { return 2; }}) * 3"), 6.0);
    assert_eq!(string("({valueOf: function () { return 2; }}) + 'x'"), "2x");
    assert_eq!(
        string(
            "({toString: function () { return 's'; }, valueOf: function () { return {}; }}) + ''"
        ),
        "s"
    );
    assert_eq!(
        num(
            "let n = 0; let a = {valueOf: function () { n = n * 10 + 1; return 8; }}; let b = {valueOf: function () { n = n * 10 + 2; return 3; }}; let r = a - b; n * 10 + r"
        ),
        125.0
    );
    assert_eq!(
        num("let o = {x: 7, valueOf: function () { return this.x; }}; +o"),
        7.0
    );
    assert_eq!(
        string("let o = {valueOf: 1, toString: function () { return 's'; }}; '' + o"),
        "s"
    );
    assert_eq!(
        num(
            "let o = {valueOf: function () { return {}; }, toString: function () { return {}; }}; let caught = 0; try { o + 1; } catch (e) { caught = 1; } caught"
        ),
        1.0
    );
    let e = failure(
        "({valueOf: function () { return {}; }, toString: function () { return {}; }}) + 1",
    );
    assert_eq!(e.kind(), VmErrorKind::TypeError);
    assert_eq!(e.message(), "cannot convert object to primitive value");
    assert_eq!(
        string("try { ({valueOf: function () { throw 'boom'; }}) + 1; } catch (e) { e; }"),
        "boom"
    );
    assert_eq!(
        failure("Object.create(null) + 1").kind(),
        VmErrorKind::TypeError
    );
    assert_eq!(
        num("function f() {} f.valueOf = function () { return 3; }; f * 2"),
        6.0
    );
    assert_eq!(string("function f() {} `${f}`"), "[object Function]");
    assert_eq!(
        string("function f() {} `${f.bind(null)}`"),
        "[object Function]"
    );
    assert_eq!(
        num("function f(a) { return this.n + a; } let o = {valueOf: f.bind({n: 4}, 3)}; o * 2"),
        14.0
    );
    assert_eq!(
        string("({valueOf: Object.prototype.toString}) + ''"),
        "[object Object]"
    );
    assert_eq!(
        num(
            "({valueOf: function () { return __rt.reduce(function (a, b) { return a + b; }, 0, __rt.vector(1, 2, 3)); }}) * 2"
        ),
        12.0
    );
    assert_eq!(
        num(
            "let n = 0; let b = {valueOf: function () { n++; return 2; }}; try { ({valueOf: function () { throw 1; }}) - b; } catch (e) {} n"
        ),
        0.0
    );
    assert_eq!(
        num(
            "let proto = {valueOf: function (arg) { if (arg === undefined) { return this.n; } return 99; }}; let o = Object.create(proto); o.n = 4; o * 2"
        ),
        8.0
    );
}

#[test]
fn object_conversions_cover_hints_operators_and_property_keys() {
    for (expr, expected) in [
        ("o - 1", 2.0),
        ("o / 2", 1.5),
        ("o % 2", 1.0),
        ("o ** 2", 9.0),
        ("o & 1", 1.0),
        ("o | 8", 11.0),
        ("o ^ 1", 2.0),
        ("o << 1", 6.0),
        ("o >> 1", 1.0),
        ("o >>> 1", 1.0),
        ("~o", -4.0),
        ("-o", -3.0),
        ("+o", 3.0),
        ("o++", 3.0),
        ("++o", 4.0),
        ("--o", 2.0),
    ] {
        assert_eq!(
            num(&format!(
                "let o = {{valueOf: function () {{ return 3; }}}}; {expr}"
            )),
            expected,
            "{expr}"
        );
    }
    for op in ["<", ">", "<=", ">="] {
        assert_eq!(
            num(&format!(
                "let n = 0; let a = {{valueOf: function () {{ n = n * 10 + 1; return 1; }}}}; let b = {{valueOf: function () {{ n = n * 10 + 2; return 2; }}}}; a {op} b; n"
            )),
            12.0
        );
    }
    assert_eq!(
        string(
            "let n = ''; let o = {toString: function () { n = n + 's'; return 'key'; }, valueOf: function () { n = n + 'v'; return 2; }}; let r = `${o}`; r + n"
        ),
        "keys"
    );
    assert_eq!(
        string(
            "let n = ''; let o = {toString: function () { n = n + 's'; return {}; }, valueOf: function () { n = n + 'v'; return 2; }}; `${o}` + n"
        ),
        "2sv"
    );
    assert_eq!(
        num(
            "let n = 0; let k = {toString: function () { n++; return 'x'; }}; let o = {x: 2}; o[k]++; o[k] += 3; let r = o[k]; n * 10 + r"
        ),
        36.0
    );
    assert_eq!(
        eval(
            "let k = {toString: function () { return 'x'; }}; let o = {}; o[k] = 7; k in o && o.x === 7 && delete o[k] && !(k in o)"
        ),
        ValueView::Bool(true)
    );
    assert_eq!(
        num(
            "let n = 0; let k = {toString: function () { n++; throw 1; }}; try { null[k]; } catch (e) {} try { k in 1; } catch (e) {} try { delete undefined[k]; } catch (e) {} n"
        ),
        0.0
    );
    assert_eq!(
        string(
            "let n = ''; let o = {valueOf: function () { n = n + 'v'; this.toString = function () { n = n + 's'; return 'ok'; }; return {}; }}; let r = '' + o; r + n"
        ),
        "okvs"
    );
}

/// Synthetic fixtures, not light-cljs output: relational operators compare
/// the primitives produced by conversion, including UTF-16 string ordering.
#[test]
fn objects_compare_after_to_primitive() {
    for (a, b, results) in [
        ("'10'", "'2'", [true, false, true, false]),
        ("'2'", "'10'", [false, true, false, true]),
        ("'10'", "2", [false, true, false, true]),
        ("2", "'10'", [true, false, true, false]),
        ("'2'", "'2'", [false, false, true, true]),
        ("undefined", "1", [false; 4]),
        ("1", "undefined", [false; 4]),
    ] {
        for (op, expected) in ["<", ">", "<=", ">="].into_iter().zip(results) {
            let src = format!(
                "let a = {{valueOf: function () {{ return {a}; }}}}; \
                 let b = {{valueOf: function () {{ return {b}; }}}}; a {op} b"
            );
            assert_eq!(eval(&src), ValueView::Bool(expected), "{src}");
        }
    }
}

/// Synthetic fixtures, not light-cljs output: constructors and receiver checks.
#[test]
fn primitive_wrapper_constructors_and_methods() {
    for src in [
        "Number('0x10') === 16 && Number() === 0 && Number(undefined) !== Number(undefined)",
        "Object.is(Number(-0), -0) && Number(null) === 0 && Number(true) === 1",
        "String() === '' && String(undefined) === 'undefined' && String(null) === 'null'",
        "String(1e21) === '1e+21' && String(-0) === '0' && String(false) === 'false'",
        "typeof new Number(1) === 'object' && typeof new String('abc') === 'object'",
        "new Number(1) !== 1 && new Number(1) !== new Number(1)",
        "new String('abc') !== new String('abc')",
        "new Number(1) != new Number(1) && new String('abc') != new String('abc')",
        "let n = new Number(1); n == n && n === n",
        "(new Number(5)).valueOf() + 1 === 6 && (new String('a')).valueOf() + 'b' === 'ab'",
        "(new Number()).valueOf() === 0 && (new String()).valueOf() === ''",
        "let n = (new Number(undefined)).valueOf(); n !== n && (new String(undefined)).valueOf() === 'undefined'",
        "(new Number('0x10')).toString() === '16' && (new String(null)).toString() === 'null'",
        "Object.is((new Number(-0)).valueOf(), -0) && (new Number(1e21)).toString(10) === '1e+21'",
        "Object.getPrototypeOf(new Number(1)) === Number.prototype && Object.getPrototypeOf(new String('x')) === String.prototype",
        "new Number(1) instanceof Number && new String('a') instanceof String && !(({}) instanceof String)",
        "Number.prototype.valueOf() === 0 && String.prototype.valueOf() === ''",
        "let n = new Number(1); n.x = 2; n.x === 2 && n.constructor === Number",
        "let s = new String('a'); s.x = 3; s.x === 3 && s.constructor === String",
        "let n = new Number(2); Object.freeze(n); Object.isFrozen(n) && n.valueOf() === 2",
        "let n = new Number(2); Object.setPrototypeOf(n, null); n.v = Number.prototype.valueOf; n.v() === 2",
        "let r = 0; let o = {v: Number.prototype.valueOf}; try { o.v(); } catch (e) { r = 1; } o.v = String.prototype.toString; try { o.v(); } catch (e) { r = r + 10; } r === 11",
        "let n = new Number(1); n.v = String.prototype.valueOf; let r = false; try { n.v(); } catch (e) { r = true; } r",
        "let s = new String('a'); s.v = Number.prototype.toString; let r = false; try { s.v(); } catch (e) { r = true; } r",
        "let n = Object.create(Number.prototype); let r = false; try { n.valueOf(); } catch (e) { r = true; } r",
        "let s = Object.create(String.prototype); let r = false; try { s.valueOf(); } catch (e) { r = true; } r",
        "let n = new Number(1); n.tag = Object.prototype.toString; n.tag() === '[object Number]'",
        "let s = new String('a'); s.tag = Object.prototype.toString; s.tag() === '[object String]'",
        "let n = new Number(3); n.valueOf = function () { return 4; }; n.valueOf() === 4 && n.toString() === '3'",
        "let s = new String('a'); s.valueOf = function () { return 'b'; }; s.valueOf() === 'b' && s.toString() === 'a'",
        r"(new String('\u{d800}')).valueOf() === '\u{d800}'",
        "let log = ''; Number(1, log = log + 'n'); new String('x', log = log + 's'); log === 'ns'",
        "Number.call(null, '0x10') === 16 && String.apply(null, [1e21]) === '1e+21'",
        "Number.prototype.valueOf.call(new Number(7)) === 7 && String.prototype.toString.apply(new String('abc'), []) === 'abc'",
        "Number.prototype.toString.call(5) === '5' && String.prototype.valueOf.call('a') === 'a'",
        "let r = 0; try { Number.prototype.valueOf.call({}); } catch (e) { r = 1; } try { String.prototype.toString.call(new Number(1)); } catch (e) { r = r + 10; } r === 11",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
}

/// String exotic properties are deferred to clj-quickjs-142.
#[test]
fn primitive_wrappers_report_deferred_string_properties() {
    // The string exotic properties belong to clj-quickjs-142. A wrapper
    // must not answer the inherited String.prototype.length = 0 or permit
    // overwriting indices while that implementation is missing.
    for src in [
        "(new String('abc')).length",
        "(new String('abc'))[0]",
        "let s = new String('abc'); s[0] = 'x'",
        "let s = new String('abc'); delete s.length",
        "Object.getOwnPropertyDescriptor(new String('abc'), '0')",
        "Object.defineProperty(new String('abc'), '0', {value: 'x'})",
        "Object.keys(new String('abc'))",
        "Object.getOwnPropertyNames(new String(''))",
        "for (let k in new String('abc')) {}",
        "'length' in new String('abc')",
        "Object.create(new String('abc')).length",
    ] {
        let error = failure(src);
        assert_eq!(
            error.kind(),
            VmErrorKind::UnsupportedOperand,
            "{src}: {error}"
        );
        assert!(error.message().contains("string wrapper"), "{src}: {error}");
    }
    for (src, column) in [
        ("let o = Object.create(null);\nNumber(o)", 7),
        ("let o = Object.create(null);\nString(o)", 7),
        ("let o = Object.create(null);\nnew Number(o)", 1),
        ("let o = Object.create(null);\nnew String(o)", 1),
    ] {
        let (kind, _, line, actual_column) = failure_at(src);
        assert_eq!(kind, VmErrorKind::TypeError);
        assert_eq!((line, actual_column), (2, column), "{src}");
    }
}

#[test]
fn primitive_wrappers_reject_inherited_string_index_writes() {
    for options in [ScriptOptions::STRICT, ScriptOptions::SLOPPY] {
        let src = "let o = Object.create(new String('abc')); o[0] = 'x'; o[0]";
        let script = compile_script_with("inherited wrapper index", src, options).unwrap();
        match run(&script, &[]) {
            RunOutcome::Failed(error) => {
                assert_eq!(error.kind(), VmErrorKind::UnsupportedOperand);
                assert!(error.message().contains("string wrapper"), "{error}");
            }
            other => panic!("expected UnsupportedOperand, got {other:?}"),
        }
        // An index at the string's length is an ordinary inherited miss,
        // so the receiver gets its own writable property in either mode.
        let src = "let o = Object.create(new String('abc')); o[3] = 'x'; o[3] === 'x'";
        let script = compile_script_with("inherited wrapper boundary", src, options).unwrap();
        match run(&script, &[]) {
            RunOutcome::Complete(value) => assert_eq!(value.view(), ValueView::Bool(true)),
            other => panic!("expected Complete, got {other:?}"),
        }
    }
}

#[test]
fn primitive_wrappers_survive_wait_collection_and_migration() {
    let rt = Runtime::new();
    let script = exe(r"let n = new Number(-0); let s = new String('\uD800x');
          n.child = {v: 7}; s.peer = n; __rt.need(72, 1);
          Object.is(s.peer.valueOf(), -0) && s.valueOf() === '\uD800x'
          && s.peer.child.v === 7 && s.toString() === '\uD800x'");
    let task = rt.spawn(&script, &[]).unwrap();
    assert!(matches!(task.run().unwrap(), RunOutcome::Waiting(_)));
    let request = task.take_producer_requests().pop().unwrap();
    rt.collect().unwrap();
    let value = rt.root(Primitive::Number(0.0));
    let wakeups = rt
        .complete(request.key(), request.producer(), Completion::Ready(&value))
        .unwrap();
    let runnable = wakeups.into_runnable().pop().unwrap();
    let result = std::thread::spawn(move || runnable.run().unwrap())
        .join()
        .unwrap();
    match result {
        RunOutcome::Complete(v) => assert_eq!(v.view(), ValueView::Bool(true)),
        other => panic!("expected Complete, got {other:?}"),
    }

    for constructor in ["new Number(7)", "new String('abc')"] {
        let owner = rt.spawn(&exe(constructor), &[]).unwrap();
        let RunOutcome::Complete(wrapper) = owner.run().unwrap() else {
            panic!("wrapper construction failed");
        };
        assert!(rt.publish(&wrapper).is_err(), "{constructor} published");
        let read = compile_function("function reader(o) { return o.valueOf(); }").unwrap();
        let intruder = rt.spawn(&read, &[&wrapper]).unwrap();
        match intruder.run().unwrap() {
            RunOutcome::Failed(e) => assert!(matches!(e.kind(), VmErrorKind::ForeignObject { .. })),
            other => panic!("expected ForeignObject, got {other:?}"),
        }
        let weak = wrapper.downgrade();
        rt.collect().unwrap();
        assert!(weak.upgrade().is_ok());
        drop((wrapper, owner, intruder));
        rt.collect().unwrap();
        assert!(weak.upgrade().is_err());
    }
}

/// The first three expressions adapt test_eq in QuickJS tests/test_language.js,
/// revision 7d1c13c5d8f8. Copyright and MIT license notice: see
/// loose_equality_with_a_boolean_left_operand above. Synthetic fixture,
/// not light-cljs output.
#[test]
fn primitive_wrappers_convert_with_value_of() {
    for src in [
        "(new Number(1)) == 1",
        "2 == (new Number(2))",
        "(new String('abc')) == 'abc'",
        "typeof new Number(1) === 'object'",
        "(new Number(1) === 1) === false",
        "(new Number(1) == new Number(1)) === false",
        "new Number(5) + 1 === 6",
        "new String('a') + 'b' === 'ab'",
        "new Number() == 0 && new String() == ''",
        "Number('0x10') === 16 && Number() === 0",
        "String(null) === 'null' && String(1e21) === '1e+21'",
        "(new String('abc') == new String('abc')) === false",
        "let n = new Number(1); n.valueOf = function () { return 7; }; n + 1 === 8 && n == 7",
        "let s = new String('a'); s.valueOf = function () { return {}; }; s.toString = function () { return 'c'; }; s + 'b' === 'cb'",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
}

/// Synthetic fixtures, not light-cljs output. Constructors use observable,
/// hint-appropriate ToPrimitive after resolving call/apply/bind forwarding.
#[test]
fn primitive_wrapper_constructors_convert_object_arguments() {
    for src in [
        "Number({valueOf: function () { return 7; }}) === 7",
        "String({toString: function () { return 's'; }}) === 's'",
        "let n = new Number({valueOf: function () { return 7; }}); typeof n === 'object' && Object.getPrototypeOf(n) === Number.prototype && n.valueOf() === 7 && n.toString() === '7' && n !== 7",
        "let s = new String({toString: function () { return 's'; }}); typeof s === 'object' && Object.getPrototypeOf(s) === String.prototype && s.valueOf() === 's' && s.toString() === 's' && s !== 's'",
        "Number(new String('0x10')) === 16 && String(new Number(1e21)) === '1e+21'",
        "let o = {valueOf: function () { return 3; }, toString: function () { return 's'; }}; Number(o) === 3 && String(o) === 's'",
        "Number({valueOf: function () { return {}; }, toString: function () { return '4'; }}) === 4",
        "String({toString: function () { return {}; }, valueOf: function () { return 4; }}) === '4'",
        "let log = ''; let o = {valueOf: function () { log = log + 'v'; return 3; }}; Number(o, log = log + 'a') === 3 && log === 'av'",
        "let o = {valueOf: function () { return 3; }}; Number.call(null, o) === 3 && Number.apply(null, [o]) === 3 && Reflect.apply(Number, null, [o]) === 3",
        "let o = {toString: function () { return 's'; }}; String.call(null, o) === 's' && String.apply(null, [o]) === 's' && Reflect.apply(String, null, [o]) === 's'",
        "let N = Number.bind(null, {valueOf: function () { return 8; }}); let n = new N(); N() === 8 && typeof n === 'object' && Object.getPrototypeOf(n) === Number.prototype && n.valueOf() === 8 && n.toString() === '8' && n !== 8",
        "let S = String.bind(null, {toString: function () { return 's'; }}); let s = new S(); S() === 's' && typeof s === 'object' && Object.getPrototypeOf(s) === String.prototype && s.valueOf() === 's' && s.toString() === 's' && s !== 's'",
        "let o = {valueOf: Number.bind(null, {valueOf: function () { return 6; }})}; +o === 6",
        "let o = {toString: String.bind(null, {toString: function () { return 's'; }})}; String(o) === 's'",
        "__rt.reduce(Number, 0, __rt.vector({valueOf: function () { return 4; }})) === 0",
        "let o = {valueOf: function () { return Number({valueOf: function () { return 6; }}); }}; Number(o) === 6",
        "let caught = false; try { Number({valueOf: function () { throw 42; }}); } catch(e) { caught = e === 42; } caught",
        "let caught = false; try { new String({toString: function () { return {}; }, valueOf: function () { return {}; }}); } catch(e) { caught = true; } caught",
        "String({}) === '[object Object]' && Number({}) !== Number({})",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
}

#[test]
fn primitive_wrapper_conversion_waits_yields_collects_and_migrates() {
    for (expression, expected) in [
        ("Number(argument())", "7"),
        ("new Number(argument())", "7"),
        ("String(argument())", "'seven'"),
        ("new String(argument())", "'seven'"),
        ("Number.call(null, argument())", "7"),
        ("String.apply(null, [argument()])", "'seven'"),
        ("Reflect.apply(Number, null, [argument()])", "7"),
        ("new (Number.bind(null, argument()))()", "7"),
        ("new (String.bind(null, argument()))()", "'seven'"),
        ("+({valueOf: Number.bind(null, argument())})", "7"),
        (
            "String({toString: String.bind(null, argument())})",
            "'seven'",
        ),
        ("__rt.reduce(Number, argument(), __rt.vector(0))", "7"),
        ("__rt.reduce(String, argument(), __rt.vector(0))", "'seven'"),
        ("Number(wrapper())", "7"),
        ("wrapper() + 1", "8"),
    ] {
        let rt = Runtime::new();
        let wrapper_check = if expression.starts_with("new ") {
            let constructor = if expression.contains("Number") {
                "Number"
            } else {
                "String"
            };
            format!(
                "typeof result === 'object' && Object.getPrototypeOf(result) === {constructor}.prototype && result.valueOf() === {expected} && result !== {expected}"
            )
        } else {
            "true".to_string()
        };
        let src = format!(
            "let calls = 0; function argument() {{ return {{n: 7, s: 'seven',
                valueOf: function () {{ calls++; __rt.need(72, 1); return this.n; }},
                toString: function () {{ calls++; __rt.need(72, 1); return this.s; }}
            }}; }}
            function wrapper() {{ let n = new Number(3); n.n = 7;
                n.valueOf = argument().valueOf; return n; }}
            let result = {expression}; result == {expected} && calls === 1 && ({wrapper_check})"
        );
        let script = exe(&src);
        let mut task = rt.spawn(&script, &[]).unwrap();
        let mut waited = false;
        let mut complete = false;
        // A finite instruction bound makes repeated conversion or a lost
        // continuation fail deterministically. Every yield is a GC boundary.
        for _ in 0..1000 {
            let (next_task, outcome) = std::thread::spawn(move || {
                let outcome = task
                    .run_with(Budget::instructions(1), &mut NoEffects)
                    .unwrap();
                (task, outcome)
            })
            .join()
            .unwrap();
            task = next_task;
            match outcome {
                RunOutcome::Yielded => {
                    rt.collect().unwrap();
                }
                RunOutcome::Waiting(_) => {
                    assert!(!waited, "repeated wait: {expression}");
                    waited = true;
                    rt.collect().unwrap();
                    let request = task.take_producer_requests().pop().unwrap();
                    let value = rt.root(Primitive::Number(0.0));
                    task = rt
                        .complete(request.key(), request.producer(), Completion::Ready(&value))
                        .unwrap()
                        .into_runnable()
                        .pop()
                        .unwrap();
                }
                RunOutcome::Complete(result) => {
                    assert_eq!(result.view(), ValueView::Bool(true), "{expression}");
                    complete = true;
                    break;
                }
                other => panic!("{expression}: {other:?}"),
            }
        }
        assert!(waited && complete, "{expression}");
    }
}

#[test]
fn primitive_wrapper_native_recursion_obeys_depth_limit() {
    let rt = Runtime::with_limits(clj_quickjs::VmLimits {
        max_call_depth: 8,
        ..clj_quickjs::VmLimits::DEFAULT
    });
    let script = exe(
        "let o = {}; o.valueOf = Number.bind(null, o); let caught = false; try { Number(o); } catch(e) { caught = true; } caught",
    );
    match rt
        .spawn(&script, &[])
        .unwrap()
        .run_with(Budget::instructions(1000), &mut NoEffects)
        .unwrap()
    {
        RunOutcome::Complete(result) => assert_eq!(result.view(), ValueView::Bool(true)),
        other => panic!("native recursion did not unwind: {other:?}"),
    }
}
