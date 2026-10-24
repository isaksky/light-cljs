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
            | Opcode::ThrowError => 2,
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

    // Valid syntax that cannot execute yet is rejected where it starts.
    for (src, line, column) in [
        ("let s = 'text';", 1, 9),
        ("f(1);", 1, 1),
        ("let o = 1;\no.p;", 2, 1),
        ("function f() {}", 1, 1),
        ("let g = function () {};", 1, 9),
        ("{ function f() {} }", 1, 3),
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
    // Near the parser's nesting budget: 60 nested parentheses and a chain of
    // 240 additions. Compiling recurses once per tree level, so it must fit
    // the same 2 MiB thread stack the parser is tested with.
    let nested = format!("{}1{}", "(".repeat(60), ")".repeat(60));
    let chain = vec!["1"; 241].join(" + ");
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
    assert_eq!(num(&vec!["1"; 241].join(" + ")), 241.0);
}
