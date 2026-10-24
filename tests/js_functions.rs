//! Functions, calls, closures and the reserved intrinsic binding compiled
//! from JavaScript source (clj-quickjs-21). Programs here are JavaScript
//! source: synthetic fixtures (`tests/fixtures/js/`, not light-cljs output),
//! one adapted QuickJS test (`quickjs_closure.js`, with its notice) and
//! inline snippets.

use clj_quickjs::frontend::{SyntaxErrorKind, compile_function, compile_script};
use clj_quickjs::{
    Budget, CollectionError, Executable, Host, HostCall, Opcode, Primitive, PublishError,
    RunOutcome, Runtime, ValueView, VmError, VmErrorKind,
};

/// Host operation 7 returns its argument plus 1000 and records the call.
#[derive(Default)]
struct PlusThousand {
    calls: Vec<f64>,
}

impl Host for PlusThousand {
    fn effect(&mut self, call: HostCall) -> Result<Primitive, String> {
        match (call.op, call.arg) {
            (7, Primitive::Number(n)) => {
                self.calls.push(n);
                Ok(Primitive::Number(n + 1000.0))
            }
            other => Err(format!("unexpected host effect {other:?}")),
        }
    }
}

fn exe(src: &str) -> Executable {
    compile_script("test", src).unwrap_or_else(|e| panic!("{src:?}: {e}"))
}

fn outcome(src: &str) -> RunOutcome {
    let rt = Runtime::new();
    let task = rt.spawn(&exe(src), &[]).unwrap();
    task.run_with(Budget::UNLIMITED, &mut PlusThousand::default())
        .unwrap()
}

fn eval(src: &str) -> ValueView {
    match outcome(src) {
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
    match outcome(src) {
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

/// Kind, line and column of a compile error.
fn compile_err(src: &str) -> (SyntaxErrorKind, u32, u32) {
    let e = compile_script("t", src).expect_err(src);
    (e.kind, e.line, e.column)
}

/// Every opcode of `exe` and of all its child functions, recursively.
fn all_opcodes(exe: &Executable) -> Vec<Opcode> {
    let code = exe.to_spec().code;
    let mut ops = Vec::new();
    let mut pos = 0;
    while pos < code.len() {
        let op = Opcode::from_byte(code[pos]).unwrap();
        ops.push(op);
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
    let mut i = 0;
    while let Some(child) = exe.function(i) {
        ops.extend(all_opcodes(&child));
        i += 1;
    }
    ops
}

fn uses(exe: &Executable, wanted: &[Opcode]) {
    let ops = all_opcodes(exe);
    for op in wanted {
        assert!(ops.contains(op), "{} missing from {ops:?}", op.name());
    }
}

#[test]
fn fixtures_run_calls_recursion_and_closures_through_vm_instructions() {
    let functions = exe(include_str!("fixtures/js/functions.js"));
    let closures = exe(include_str!("fixtures/js/closures.js"));
    let quickjs = exe(include_str!("fixtures/js/quickjs_closure.js"));
    let run = |exe: &Executable| match Runtime::new().spawn(exe, &[]).unwrap().run().unwrap() {
        RunOutcome::Complete(v) => v.view(),
        other => panic!("{other:?}"),
    };
    assert_eq!(run(&functions), ValueView::Number(11154.0));
    assert_eq!(run(&closures), ValueView::Number(131070.0));
    // 0: every adapted QuickJS check passed.
    assert_eq!(run(&quickjs), ValueView::Number(0.0));

    // The programs are lowered to calls and closures, not interpreted
    // otherwise: child code, `function`, `call`, `current_function` (fact's
    // self-reference), captures and binding cells.
    assert!(functions.function(0).is_some());
    uses(
        &functions,
        &[Opcode::Function, Opcode::Call, Opcode::CurrentFunction],
    );
    uses(
        &closures,
        &[
            Opcode::Function,
            Opcode::Call,
            Opcode::GetCapture,
            Opcode::NewCell,
            Opcode::CellInit,
            Opcode::CellGet,
            Opcode::CellSet,
        ],
    );
}

#[test]
fn intrinsic_fixture_runs_collection_and_effect_intrinsics() {
    let exe = exe(include_str!("fixtures/js/intrinsics.js"));
    let rt = Runtime::new();
    let task = rt.spawn(&exe, &[]).unwrap();
    let mut host = PlusThousand::default();
    let RunOutcome::Complete(v) = task.run_with(Budget::UNLIMITED, &mut host).unwrap() else {
        panic!("intrinsics fixture did not complete")
    };
    assert_eq!(v.view(), ValueView::Number(1342.0));
    assert_eq!(host.calls, [20.0]);
    uses(
        &exe,
        &[
            Opcode::NewMap,
            Opcode::Count,
            Opcode::Nth,
            Opcode::Get,
            Opcode::GetOr,
            Opcode::Assoc,
            Opcode::Dissoc,
            Opcode::Conj,
            Opcode::NewVectorBuilder,
            Opcode::BuilderConj,
            Opcode::Freeze,
            Opcode::HostEffect,
        ],
    );
    // No intrinsic is a call: `__rt` is never loaded as a value.
    let ops = all_opcodes(&exe);
    let calls = ops.iter().filter(|&&op| op == Opcode::Call).count();
    assert_eq!(calls, 3, "only the three user-function calls: {ops:?}");

    // The remaining intrinsics, and run-time operand errors.
    assert_eq!(num("__rt.count(__rt.popLast(__rt.vector(1, 2, 3)))"), 2.0);
    assert_eq!(
        num("const b = __rt.mapBuilder(); __rt.builderAssoc(b, 1, 5); __rt.get(__rt.freeze(b), 1)"),
        5.0
    );
    assert_eq!(num("__rt.count(__rt.vector())"), 0.0);
    let (kind, _, line, column) = failure_at("let n = 1;\n__rt.count(n)");
    assert_eq!(
        (kind, line, column),
        (
            VmErrorKind::Collection(CollectionError::WrongType {
                expected: "vector or map",
                found: "number"
            }),
            2,
            1
        )
    );
}

#[test]
fn calls_evaluate_callee_then_arguments_and_bind_parameters() {
    // Order: callee, then arguments left to right, then the call.
    assert_eq!(
        num("let log = 0;
             function note(d, v) { log = log * 10 + d; return v; }
             function add3(a, b, c) { return a + b + c; }
             note(1, add3)(note(2, 10), note(3, 20), note(4, 30)) * 10000 + log"),
        60_1234.0
    );
    // Missing arguments are undefined; extra ones are evaluated and ignored.
    assert_eq!(
        eval("function f(a, b) { return b; } f(1)"),
        ValueView::Undefined
    );
    assert_eq!(
        num("let n = 0; function f(a) { return a; } f(1, n = 5) + n"),
        6.0
    );
    // A function without `return` returns undefined; a call is an expression.
    assert_eq!(eval("function f() { 1; } f()"), ValueView::Undefined);
    // Each evaluation of a function expression is a new object.
    assert_eq!(
        eval("function mk() { return function () {}; } mk() === mk()"),
        ValueView::Bool(false)
    );
    assert_eq!(
        eval("function f() {} const g = f; g === f"),
        ValueView::Bool(true)
    );
    // Parameters are locals of the call: assigning one changes nothing
    // outside.
    assert_eq!(
        num("let x = 1; function f(x) { x = 9; return x; } f(x) + x"),
        10.0
    );
}

#[test]
fn call_failures_are_type_and_reference_errors_at_the_call() {
    let (kind, message, line, column) = failure_at("let x = 1;\nx(2)");
    assert_eq!(
        (kind, message.as_str(), line, column),
        (VmErrorKind::TypeError, "number is not a function", 2, 1)
    );
    let (kind, message, line, column) = failure_at("let a = 1;\nmissing(a)");
    assert_eq!(
        (kind, message.as_str(), line, column),
        (VmErrorKind::ReferenceError, "missing is not defined", 2, 1)
    );
    // Unbounded recursion fails the task; no Rust stack is involved.
    let e = failure("function f(n) { return f(n + 1); } f(0)");
    assert_eq!(e.kind(), VmErrorKind::StackOverflow);
}

#[test]
fn hoisted_declarations_and_self_references() {
    // Declarations are created when their block is entered, in order.
    assert_eq!(num("{ const r = g(); function g() { return 7; } r }"), 7.0);
    // A declaration's own name inside it is the running function...
    let exe = exe("function f(n) { return n > 0 ? f(n - 1) : 42; } f(3)");
    assert_eq!(
        all_opcodes(&exe.function(0).unwrap())
            .iter()
            .filter(|&&o| o == Opcode::GetCapture)
            .count(),
        0
    );
    uses(&exe, &[Opcode::CurrentFunction]);
    // ...unless the binding is reassigned: then it is the binding.
    assert_eq!(
        num("function f(n) { return n === 0 ? 0 : 1 + f(n - 1); }
             const orig = f;
             f = function (n) { return 100; };
             orig(3)"),
        101.0
    );
    // A named function expression's name is immutable: assigning it inside
    // is a TypeError (strict mode), and it does not leak outside.
    let (kind, message, ..) = failure_at("const g = function h() { h = 1; };\ng()");
    assert_eq!(
        (kind, message.as_str()),
        (
            VmErrorKind::TypeError,
            "assignment to constant variable 'h'"
        )
    );
    assert_eq!(
        failure("const g = function h() {}; h").kind(),
        VmErrorKind::ReferenceError
    );
    // Shadowing a function's name inside it is an ordinary binding.
    assert_eq!(num("const g = function h(h) { return h * 2; }; g(4)"), 8.0);
}

#[test]
fn closures_see_assignments_and_keep_bindings_alive() {
    // Two closures share one binding.
    assert_eq!(
        num("function pair() {
               let v = 1;
               return __rt.vector(function (x) { v = x; }, function () { return v; });
             }
             const p = pair();
             __rt.nth(p, 0)(41);
             __rt.nth(p, 1)() + 1"),
        42.0
    );
    // A closure sees assignments made after it was created, in the frame
    // and in other closures, and the frame sees the closure's.
    assert_eq!(
        num("let a = 1;
             const get = function () { return a; };
             a = 5;
             const set = function (x) { a = x; };
             set(get() * 2);
             a"),
        10.0
    );
    // Captured parameters are bindings too.
    assert_eq!(
        num("function acc(total) {
               return function (x) { total = total + x; return total; };
             }
             const f = acc(100);
             f(1);
             f(2)"),
        103.0
    );
    // Deep nesting: a binding three functions out.
    assert_eq!(
        num("function a(x) {
               return function b(y) {
                 return function c(z) { x = x + 1; return x * 100 + y * 10 + z; };
               };
             }
             const c = a(1)(2);
             c(3);
             c(3)"),
        323.0
    );
}

#[test]
fn escaping_closures_survive_yields_and_collections() {
    let src = include_str!("fixtures/js/closures.js");
    let exe = exe(src);
    let rt = Runtime::new();
    let task = rt.spawn(&exe, &[]).unwrap();
    let mut yields = 0;
    let result = loop {
        match task
            .run_with(Budget::instructions(5), &mut PlusThousand::default())
            .unwrap()
        {
            RunOutcome::Yielded => {
                yields += 1;
                rt.collect().unwrap();
            }
            RunOutcome::Complete(v) => break v.view(),
            other => panic!("{other:?}"),
        }
    };
    assert_eq!(result, ValueView::Number(131070.0));
    assert!(yields > 20, "only {yields} yields");
}

#[test]
fn closures_check_the_temporal_dead_zone_at_run_time() {
    // A closure that runs before the declaration: ReferenceError at the
    // reference inside the closure.
    let (kind, message, line, column) =
        failure_at("function g() {\n  return x;\n}\ng();\nlet x = 1;");
    assert_eq!(
        (kind, message.as_str(), line, column),
        (
            VmErrorKind::ReferenceError,
            "cannot access 'x' before initialization",
            2,
            10
        )
    );
    let (kind, _, line, column) =
        failure_at("const f = function () { return c; };\nf();\nconst c = 1;");
    assert_eq!((kind, line, column), (VmErrorKind::ReferenceError, 1, 32));
    // Assigning through a closure before initialization is also a
    // ReferenceError, even for a `const` (before its TypeError).
    let (kind, _, line, column) = failure_at("function s() { y = 2; }\ns();\nlet y;");
    assert_eq!((kind, line, column), (VmErrorKind::ReferenceError, 1, 16));
    let (kind, ..) = failure_at("function s() { c = 2; }\ns();\nconst c = 1;");
    assert_eq!(kind, VmErrorKind::ReferenceError);
    // After the declaration ran, the same closures work.
    assert_eq!(num("function g() { return x; }\nlet x = 1;\ng()"), 1.0);
    let (kind, message, ..) = failure_at("const c = 1;\nfunction s() { c = 2; }\ns()");
    assert_eq!(
        (kind, message.as_str()),
        (
            VmErrorKind::TypeError,
            "assignment to constant variable 'c'"
        )
    );
    // Inside its own function, position still decides statically.
    let (kind, _, line, column) = failure_at("function f() { let a = b; let b = 1; }\nf()");
    assert_eq!((kind, line, column), (VmErrorKind::ReferenceError, 1, 24));
    // A closure in its own initializer reads it later, initialized.
    assert_eq!(
        eval("let f = function () { return f; };\nf() === f"),
        ValueView::Bool(true)
    );
    // Mutual recursion between hoisted declarations is fine.
    assert_eq!(
        eval(
            "{ function even(n) { return n === 0 || odd(n - 1); }\nfunction odd(n) { return n !== 0 && even(n - 1); }\neven(6) }"
        ),
        ValueView::Bool(true)
    );
}

#[test]
fn closures_over_immutable_bindings_capture_values_and_are_publishable() {
    let rt = Runtime::new();
    // `k` and `n` are never assigned and initialized before the closures
    // exist, so they are captured by value: no cells. (A hoisted declaration
    // would be created before `k` is initialized and would need a cell.)
    let src = "const k = 3;
               const scale = function (n) { return function (x) { return x * n * k; }; };
               scale(2)";
    assert!(
        all_opcodes(&exe("const k = 3; function f() { return k; } f")).contains(&Opcode::NewCell)
    );
    let scaled = exe(src);
    assert!(!all_opcodes(&scaled).contains(&Opcode::NewCell));
    let task = rt.spawn(&scaled, &[]).unwrap();
    let RunOutcome::Complete(f) = task.run().unwrap() else {
        panic!()
    };
    rt.publish(&f)
        .expect("a closure over immutable values is shareable");
    // Called from another task.
    let apply = compile_function("function apply(f) { return f(5); }").unwrap();
    let other = rt.spawn(&apply, &[&f]).unwrap();
    let RunOutcome::Complete(v) = other.run().unwrap() else {
        panic!()
    };
    assert_eq!(v.view(), ValueView::Number(30.0));

    // A closure over an assigned binding holds a task-owned cell.
    let counter = exe("let n = 0; function inc() { n = n + 1; return n; } inc");
    let task = rt.spawn(&counter, &[]).unwrap();
    let RunOutcome::Complete(inc) = task.run().unwrap() else {
        panic!()
    };
    let err = rt.publish(&inc).unwrap_err();
    assert!(
        matches!(
            err,
            PublishError::NotShareable {
                found: "binding cell",
                ..
            }
        ),
        "{err:?}"
    );
    // Called by another task anyway, its binding is foreign.
    let other = rt.spawn(&apply, &[&inc]).unwrap();
    let RunOutcome::Failed(e) = other.run().unwrap() else {
        panic!()
    };
    assert!(
        matches!(e.kind(), VmErrorKind::ForeignBinding { .. }),
        "{e}"
    );
}

#[test]
fn reserved_binding_misuse_is_a_compile_error() {
    use SyntaxErrorKind::{Invalid, ReservedWord, Unsupported};
    let cases = [
        // Shadowing: no declaration may bind the reserved name.
        ("let __rt = 1;", ReservedWord, 1, 5),
        ("const __rt = 1;", ReservedWord, 1, 7),
        ("function f(__rt) {}", ReservedWord, 1, 12),
        ("function __rt() {}", ReservedWord, 1, 10),
        ("let g = function __rt() {};", ReservedWord, 1, 18),
        ("function f() {\n  { let __rt; }\n}", ReservedWord, 2, 9),
        // Reassignment of the binding or of an intrinsic.
        ("__rt = 1;", ReservedWord, 1, 1),
        ("function f() { __rt = 1; }", ReservedWord, 1, 16),
        ("__rt.vector = 1;", ReservedWord, 1, 1),
        // Computed access.
        ("__rt['vector'](1);", ReservedWord, 1, 5),
        ("let i = 0; __rt[i];", ReservedWord, 1, 16),
        // Aliasing: the binding and its intrinsics are not values.
        ("const r = __rt;", ReservedWord, 1, 11),
        ("function f(x) {} f(__rt);", ReservedWord, 1, 20),
        ("function f() { return __rt; }", ReservedWord, 1, 23),
        ("const v = __rt.vector;", ReservedWord, 1, 11),
        (
            "const g = function () { return (__rt).count; };",
            ReservedWord,
            1,
            32,
        ),
        ("__rt.vector.call(1);", Unsupported, 1, 1),
        // Unknown intrinsics and wrong arguments.
        ("__rt.toString();", Invalid, 1, 6),
        ("__rt.nth(__rt.vector());", Invalid, 1, 1),
        ("__rt.map(1);", Invalid, 1, 1),
        ("let op = 1; __rt.effect(op, 2);", Invalid, 1, 25),
        ("__rt.effect(70000, 2);", Invalid, 1, 13),
        ("__rt.effect(1.5, 2);", Invalid, 1, 13),
        // `__rt.need` (lowered by clj-quickjs-23): arity and literal identities.
        ("const h = __rt.need(1);", Invalid, 1, 11),
        ("const h = __rt.need(1.5, 2);", Invalid, 1, 21),
    ];
    for (src, kind, line, column) in cases {
        assert_eq!(compile_err(src), (kind, line, column), "{src}");
    }
    // The diagnostics name the contract.
    let message = |src: &str| compile_script("t", src).unwrap_err().message;
    assert!(message("let __rt;").contains("cannot be declared or shadowed"));
    assert!(message("__rt = 1;").contains("cannot be assigned"));
    assert!(message("__rt[0];").contains("computed access"));
    assert!(message("const r = __rt;").contains("is not a value"));
    assert!(message("const v = __rt.vector;").contains("must be called directly"));
    assert!(message("__rt.nth(1);").contains("takes 2 arguments, got 1"));
    assert!(message("__rt.need(1, 0.5);").contains("must be an integer"));
    // The entry function of `compile_function` is a declaration too.
    let e = compile_function("function __rt() {}").unwrap_err();
    assert_eq!(e.kind, ReservedWord);
}

#[test]
fn intrinsics_are_resolved_by_the_reserved_binding_not_by_spelling() {
    // User functions named like intrinsics are ordinary functions.
    assert_eq!(
        num("function vector(a, b) { return a - b; } vector(5, 2)"),
        3.0
    );
    assert_eq!(
        num("function count(v) { return -1; } count(__rt.vector(1, 2))"),
        -1.0
    );
    // Other names that merely start with `__rt` are ordinary bindings.
    assert_eq!(num("let __rt2 = 4; __rt2 * 2"), 8.0);
    // `__rt` is reserved in every function, including nested ones.
    assert_eq!(
        num("function f() { return function () { return __rt.count(__rt.vector(1)); }; } f()()"),
        1.0
    );
}

#[test]
fn deeply_nested_functions_compile_within_a_small_stack() {
    // Within the parser's nesting budget: 40 nested function declarations,
    // the innermost capturing the outermost parameter through every level,
    // and 20 nested, immediately called function expressions. Compiling
    // recurses per level, so it must fit the 2 MiB thread stack the parser
    // is tested with.
    let mut decls = String::from("function f0(x) { ");
    for i in 1..40 {
        decls.push_str(&format!("function f{i}() {{ "));
    }
    decls.push_str("return x;");
    for i in (1..40).rev() {
        decls.push_str(&format!(" }} return f{i}();"));
    }
    decls.push_str(" } f0(7)");
    let exprs = format!(
        "{}9{}",
        "(function () { return ".repeat(20),
        "; })()".repeat(20)
    );
    let handle = std::thread::Builder::new()
        .stack_size(2 << 20)
        .spawn(move || {
            let a = compile_script("decls", &decls);
            let b = compile_script("exprs", &exprs);
            (a, b)
        })
        .unwrap();
    let (a, b) = handle.join().unwrap();
    let run = |exe: Executable| match Runtime::new().spawn(&exe, &[]).unwrap().run().unwrap() {
        RunOutcome::Complete(v) => v.view(),
        other => panic!("{other:?}"),
    };
    assert_eq!(run(a.unwrap()), ValueView::Number(7.0));
    assert_eq!(run(b.unwrap()), ValueView::Number(9.0));
}

#[test]
fn redeclaration_rules_cover_function_declarations() {
    use SyntaxErrorKind::{Invalid, Unsupported};
    assert_eq!(compile_err("let f;\nfunction f() {}"), (Invalid, 2, 10));
    assert_eq!(
        compile_err("function f() { const g = 1; function g() {} }"),
        (Invalid, 1, 38)
    );
    assert_eq!(
        compile_err("function f(a) { function a() {} }"),
        (Unsupported, 1, 26)
    );
    // Separate blocks and functions are separate scopes.
    assert_eq!(
        num("function g() { return 1; } { function g() { return 2; } } g()"),
        1.0
    );
}

#[test]
fn documented_demo_programs_run_from_javascript_source() {
    // `cargo run` (src/main.rs) runs examples/demo.js.
    let demo = exe(include_str!("../examples/demo.js"));
    let rt = Runtime::new();
    let mut host = EchoHost::default();
    let task = rt.spawn(&demo, &[]).unwrap();
    let RunOutcome::Complete(v) = task.run_with(Budget::UNLIMITED, &mut host).unwrap() else {
        panic!("demo did not complete")
    };
    assert_eq!(v.view(), ValueView::Number(89.0));
    assert_eq!(host.effects, [(0, 11.0)]);

    // `cargo run --example resumable_counter` runs
    // examples/resumable_counter.js with a budget of 5 instructions per run;
    // every effect runs exactly once across the yields.
    let count = compile_function(include_str!("../examples/resumable_counter.js")).unwrap();
    let n = rt.root(Primitive::Number(3.0));
    let task = rt.spawn(&count, &[&n]).unwrap();
    let mut host = EchoHost::default();
    let mut yields = 0;
    let result = loop {
        match task.run_with(Budget::instructions(5), &mut host).unwrap() {
            RunOutcome::Yielded => yields += 1,
            RunOutcome::Complete(v) => break v.view(),
            other => panic!("{other:?}"),
        }
    };
    assert_eq!(result, ValueView::Number(2.0));
    assert_eq!(host.effects, [(0, 0.0), (0, 1.0), (0, 2.0)]);
    assert!(yields > 5, "only {yields} yields");
}

/// Records each effect and returns its argument.
#[derive(Default)]
struct EchoHost {
    effects: Vec<(u16, f64)>,
}

impl Host for EchoHost {
    fn effect(&mut self, call: HostCall) -> Result<Primitive, String> {
        let Primitive::Number(n) = call.arg else {
            return Err(format!("unexpected argument {:?}", call.arg));
        };
        self.effects.push((call.op, n));
        Ok(call.arg)
    }
}

#[test]
fn captured_assignment_expressions_return_the_assigned_value() {
    // A cell assigned in its own frame (the script) and through a closure:
    // the assignment expression's value is the assigned value, and the cell
    // holds it.
    let src = "let x = 0;
               function set(v) { return x = v; }
               const r = set(42);
               const s = (x = 7);
               r * 1000 + s * 10 + x";
    uses(&exe(src), &[Opcode::CellSet]);
    assert_eq!(num(src), 42_077.0);
    // A captured parameter assigned inside a nested closure.
    assert_eq!(
        num(
            "function f(p) { const g = function () { return p = p + 1; }; return g() * 10 + p; } f(4)"
        ),
        55.0
    );
}

#[test]
fn loop_iterations_allocate_distinct_captured_cells() {
    // Each iteration's `j` is a separate mutable cell; reusing one cell
    // would make the result 203.
    let src = "let i = 0;
               let first, last;
               while (i < 2) {
                 let j = i;
                 const bump = function () { j = j + 1; return j; };
                 if (i === 0) { first = bump; }
                 last = bump;
                 i = i + 1;
               }
               first() * 100 + last()";
    uses(&exe(src), &[Opcode::NewCell, Opcode::CellSet]);
    assert_eq!(num(src), 102.0);
}

#[test]
fn intrinsic_arguments_evaluate_left_to_right() {
    // A counted constructor.
    assert_eq!(
        num("let n = 0; const v = __rt.vector(n = 1, n = 2);
             __rt.nth(v, 0) * 100 + __rt.nth(v, 1) * 10 + n"),
        122.0
    );
    // Key/value pairs.
    assert_eq!(
        num(
            "let n = 0; const m = __rt.map(n = 1, n * 10, n = 2, n * 20);
             __rt.get(m, 1) + __rt.get(m, 2) + n"
        ),
        52.0
    );
    // A fixed-arity operation: the collection operand is evaluated first.
    assert_eq!(
        num("let log = 0;
             function note(d, v) { log = log * 10 + d; return v; }
             __rt.assoc(note(1, __rt.vector(0)), note(2, 0), note(3, 9));
             log"),
        123.0
    );
}
