//! Functions, calls, closures and the reserved intrinsic binding compiled
//! from JavaScript source (clj-quickjs-21). Programs here are JavaScript
//! source: synthetic fixtures (`tests/fixtures/js/`, not light-cljs output),
//! one adapted QuickJS test (`quickjs_closure.js`, with its notice) and
//! inline snippets.

use clj_quickjs::frontend::{
    ScriptOptions, SyntaxErrorKind, compile_function, compile_script, compile_script_with,
};
use clj_quickjs::{
    Budget, CollectionError, Completion, DependencyId, DependencyKey, Executable, Host, HostCall,
    Opcode, Primitive, PublishError, ResultKind, RunOutcome, Runtime, ValueView, VmError,
    VmErrorKind, VmLimits,
};

/// Host operation 7 returns its argument plus 1000 and records the call.
#[derive(Default)]
struct PlusThousand {
    calls: Vec<f64>,
}

impl Host for PlusThousand {
    fn effect(&mut self, call: HostCall) -> Result<Primitive, String> {
        match (call.op, &call.arg) {
            (7, &Primitive::Number(n)) => {
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

/// Synthetic fixtures, not light-cljs output.
#[test]
fn bound_functions() {
    assert_eq!(
        num(
            "function f(a,b) { return __rt.vector(this,a,b); } const g=f.bind(1,2); const v=g(3); __rt.nth(v,0)*100+__rt.nth(v,1)*10+__rt.nth(v,2);"
        ),
        123.0
    );
    assert_eq!(
        eval(
            "function f(a,b) {} const g=f.bind(1,2); g.length===1 && g.name==='bound f' && g.bind(2).name==='bound bound f';"
        ),
        ValueView::Bool(true)
    );
    assert_eq!(
        num("function C(a) { this.x=a; } const B=C.bind(null,1); new B().x;"),
        1.0
    );
    assert_eq!(
        num("function f(a,b) { return this*100+a*10+b; } f.bind(1).bind(2,3)(4);"),
        134.0
    );
    assert_eq!(
        failure("Function.prototype.bind.call(1);").kind(),
        VmErrorKind::TypeError
    );
    assert_eq!(
        eval(
            "function C(a) { this.x=a; } const B=C.bind(null,7).bind(null); const o=new B(); o.x===7 && o instanceof B && o instanceof C && B.prototype===undefined && typeof B==='function';"
        ),
        ValueView::Bool(true)
    );
    let rt = Runtime::new();
    let task = rt
        .spawn(
            &exe("const n=4; const g=(function f(a,b) { return n+a+b; }).bind(null,3); Object.getOwnPropertyDescriptor(g,'length'); Object.getOwnPropertyNames(g); g;"),
            &[],
        )
        .unwrap();
    let RunOutcome::Complete(bound) = task.run().unwrap() else {
        panic!()
    };
    drop(task);
    rt.publish(&bound).unwrap();
    rt.collect().unwrap();
    let call = compile_function(
        "function call(f) { if (f.name !== 'bound f' || f.length !== 1) return -1; return f(2); }",
    )
    .unwrap();
    let RunOutcome::Complete(v) = rt.spawn(&call, &[&bound]).unwrap().run().unwrap() else {
        panic!()
    };
    assert_eq!(v.view(), ValueView::Number(9.0));
    for src in [
        "let n=0; (function () { n=n+1; return n; }).bind(null);",
        "(function () {}).bind({});",
        "(function () {}).bind(null, {});",
        "const g=(function () {}).bind(); g.x=1; g;",
    ] {
        let RunOutcome::Complete(f) = rt.spawn(&exe(src), &[]).unwrap().run().unwrap() else {
            panic!()
        };
        assert!(
            matches!(rt.publish(&f), Err(PublishError::NotShareable { .. })),
            "{src}"
        );
    }
}

#[test]
fn bound_function_properties_and_native_callbacks() {
    for src in [
        "function f(a,b) {} const g=f.bind(null,1,2,3); const d=Object.getOwnPropertyDescriptor(g,'length'); g.length===0 && !d.writable && !d.enumerable && d.configurable && Object.getOwnPropertyNames(g)[0]==='length' && Object.getOwnPropertyNames(g)[1]==='name';",
        "function f() {} const g=f.bind(); let r=false; try { g.name='x'; } catch(e) { r=true; } r && g.name==='bound f' && delete g.name && g.name==='';",
        "function f() {} const g=f.bind(); Object.defineProperty(g,'name',{value:'x'}); g.name==='x' && g.bind().name==='bound x';",
        "const f=(function() {}).bind(); Object.defineProperty(f,'length',{value:4.8}); Object.defineProperty(f,'name',{value:7}); const g=f.bind(null,1); g.length===3 && g.name==='bound ' && Object.getPrototypeOf(g)===Function.prototype;",
        "Object.is.bind(null,1)(1) && Object.prototype.toString.call(Object.is.bind())==='[object Function]';",
        "function f(k, acc, x) { return k+acc+x; } __rt.reduce(f.bind(null,10),0,__rt.vector(1,2))===23;",
        "function f(a,b) { return this+a+b; } const g=f.bind(1,2); g.call(7,3)===6 && g.apply(8,[3])===6 && Reflect.apply(g,9,[3])===6;",
        "function f() { return this; } f.bind()()===undefined;",
        "function C(x) { this.x=x; } const receiver={x:0}; const B=C.bind(receiver,7); const o=new B(); receiver.x===0 && o.x===7;",
        "function C() { return {x:9}; } (new (C.bind(null))()).x===9;",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
    assert_eq!(
        failure("new ((a)=>a).bind(null,1)();").kind(),
        VmErrorKind::TypeError
    );
}

#[test]
fn bound_calls_and_constructors_suspend_and_migrate() {
    for (src, expected_value) in [
        (
            "function f(a) { return __rt.need(5,3)*this.k+a; } f.bind({k:10},2)();",
            42.0,
        ),
        (
            "function C(a) { this.x=__rt.need(5,3)+a; } const B=C.bind({x:0},2); (new B()).x;",
            6.0,
        ),
        (
            "function f(k,acc,x) { return __rt.need(5,3)+k+acc+x; } __rt.reduce(f.bind(null,10),0,__rt.vector(1,2));",
            31.0,
        ),
    ] {
        let rt = Runtime::new();
        let task = rt.spawn(&exe(src), &[]).unwrap();
        assert!(matches!(task.run().unwrap(), RunOutcome::Waiting(_)));
        rt.collect().unwrap();
        let requests = task.take_producer_requests();
        assert_eq!(requests.len(), 1);
        let request = &requests[0];
        let value = rt.root(Primitive::Number(4.0));
        let mut tasks = rt
            .complete(request.key(), request.producer(), Completion::Ready(&value))
            .unwrap()
            .into_runnable();
        assert_eq!(tasks.len(), 1);
        let task = tasks.pop().unwrap();
        let RunOutcome::Complete(v) = std::thread::spawn(move || task.run().unwrap())
            .join()
            .unwrap()
        else {
            panic!("{src}")
        };
        assert_eq!(v.view(), ValueView::Number(expected_value));
    }
}

#[test]
fn bound_argument_and_hop_limits() {
    let rt = Runtime::with_limits(VmLimits {
        max_call_depth: 2,
        max_stack_values: 100,
    });
    let task = rt
        .spawn(&exe("function f(x) { return x; } f.bind(null,7)();"), &[])
        .unwrap();
    let RunOutcome::Complete(value) = task.run().unwrap() else {
        panic!("one bound hop must fit")
    };
    assert_eq!(value.view(), ValueView::Number(7.0));
    for src in [
        "function main() { function f() {} return f.bind(null,1,2,3,4).bind(null,5,6,7,8)(); }",
        "function main() { function C() {} const B=C.bind(null,1,2,3,4).bind(null,5,6,7,8); return new B(); }",
    ] {
        let rt = Runtime::with_limits(VmLimits {
            max_call_depth: 100,
            max_stack_values: 8,
        });
        let task = rt.spawn(&compile_function(src).unwrap(), &[]).unwrap();
        assert!(
            matches!(task.run().unwrap(),RunOutcome::Failed(e) if e.kind()==VmErrorKind::StackOverflow),
            "{src}"
        );
    }
    let rt = Runtime::with_limits(VmLimits {
        max_call_depth: 3,
        max_stack_values: 100,
    });
    let task = rt
        .spawn(
            &exe("function f() {} let g=f; for(let i=0;i<10;i++) { g=g.bind(); } g();"),
            &[],
        )
        .unwrap();
    assert!(
        matches!(task.run().unwrap(),RunOutcome::Failed(e) if e.kind()==VmErrorKind::StackOverflow)
    );
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
            | Opcode::GetCapture
            | Opcode::GetField
            | Opcode::PutField
            | Opcode::DefineField => 2,
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
    // At the call's `(` (clj-quickjs-52).
    let (kind, _, line, column) = failure_at("let n = 1;\n__rt.count(n)");
    assert_eq!(
        (kind, line, column),
        (
            VmErrorKind::Collection(CollectionError::WrongType {
                expected: "vector or map",
                found: "number"
            }),
            2,
            11
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
    // A call fails at its `(` (clj-quickjs-52); an unknown callee name at
    // the name.
    let (kind, message, line, column) = failure_at("let x = 1;\nx(2)");
    assert_eq!(
        (kind, message.as_str(), line, column),
        (VmErrorKind::TypeError, "number is not a function", 2, 2)
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
    use SyntaxErrorKind::{Invalid, ReservedWord};
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
        // Since method calls compile (clj-quickjs-117) the uncalled
        // intrinsic is the error.
        ("__rt.vector.call(1);", ReservedWord, 1, 1),
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
    // Called function expressions nested in default values
    // (clj-quickjs-100), as deep as the parser allows, the innermost
    // default reading the outermost parameter.
    let defaults = |n: usize| {
        format!(
            "(function (x = 5) {{ return {}x{}; }})()",
            "(function (a = ".repeat(n),
            ") { return a; })()".repeat(n)
        )
    };
    let handle = std::thread::Builder::new()
        .stack_size(2 << 20)
        .spawn(move || {
            let a = compile_script("decls", &decls);
            let b = compile_script("exprs", &exprs);
            let c = (1..100)
                .map(|n| (n, compile_script("defaults", &defaults(n))))
                .take_while(|(_, c)| !matches!(c, Err(e) if e.kind == SyntaxErrorKind::TooDeep))
                .last()
                .unwrap();
            (a, b, c)
        })
        .unwrap();
    let (a, b, (depth, c)) = handle.join().unwrap();
    let run = |exe: Executable| match Runtime::new().spawn(&exe, &[]).unwrap().run().unwrap() {
        RunOutcome::Complete(v) => v.view(),
        other => panic!("{other:?}"),
    };
    assert_eq!(run(a.unwrap()), ValueView::Number(7.0));
    assert_eq!(run(b.unwrap()), ValueView::Number(9.0));
    assert!(depth >= 15, "only {depth} nested defaults");
    assert_eq!(run(c.unwrap()), ValueView::Number(5.0));
}

#[test]
fn redeclaration_rules_cover_function_declarations() {
    use SyntaxErrorKind::Invalid;
    assert_eq!(compile_err("let f;\nfunction f() {}"), (Invalid, 2, 10));
    assert_eq!(
        compile_err("function f() { const g = 1; function g() {} }"),
        (Invalid, 1, 38)
    );
    // Separate blocks and functions are separate scopes.
    assert_eq!(
        num("function g() { return 1; } { function g() { return 2; } } g()"),
        1.0
    );
}

/// Function declarations directly in a script or function body are
/// var-scoped (clj-quickjs-94): they share one binding with a parameter, a
/// `var` or another declaration of the same name, the last declaration
/// wins, and all are created at entry. Block-level ones stay lexical.
#[test]
fn top_level_function_declarations_are_var_scoped() {
    assert_eq!(
        num("function f(a) { function a() { return 7; } return a(); } f(1)"),
        7.0
    );
    assert_eq!(
        num("function g() { return 1; } function g() { return 2; } g()"),
        2.0
    );
    assert_eq!(num("var h; function h() { return 3; } h()"), 3.0);
    assert_eq!(
        compile_err("{ function k() {} function k() {} }"),
        (SyntaxErrorKind::Invalid, 1, 28)
    );
    // `var h;` after the function does not reset it; a `var` initializer
    // runs in order, after the hoisted function.
    assert_eq!(num("function h() { return 3; } var h; h()"), 3.0);
    assert_eq!(num("var h = 4; function h() { return 3; } h"), 4.0);
    assert_eq!(
        num(
            "function f() { const r = h(); var h = 5; function h() { return 6; } return r * 10 + h; } f()"
        ),
        65.0
    );
    // An earlier hoisted closure sees the winning declaration, through a
    // cell (the binding is assigned at entry), also in a function body.
    let src = "function f(a) {
                 function g() { return a() + b(); }
                 function a() { return 10; }
                 function b() { return 1; }
                 function b() { return 2; }
                 return g();
               }
               f(1)";
    uses(&exe(src), &[Opcode::NewCell, Opcode::CellInit]);
    assert_eq!(num(src), 12.0);
    // A repeated declaration refers to the binding, not to itself.
    assert_eq!(
        num("function r(n) { return -1; }
             function r(n) { return n === 0 ? 10 : r(n - 1) + 1; }
             r(3)"),
        13.0
    );
    assert_eq!(
        num("function p() { return p; } function p() { return 8; } p()"),
        8.0
    );
    // A single declaration still refers to itself without a capture.
    let single = exe("function q(n) { return n === 0 ? 0 : q(n - 1); } var q; q(2)");
    uses(&single, &[Opcode::CurrentFunction]);
    assert!(!all_opcodes(&single).contains(&Opcode::NewCell));
    // In a block, a function declaration is lexical: it cannot share a
    // name with a `var` there, and does not replace the outer binding.
    assert_eq!(
        compile_err("function f() { { var m; function m() {} } }"),
        (SyntaxErrorKind::Invalid, 1, 22)
    );
    assert_eq!(
        num("function f(a) { { function a() { return 9; } } return a; } f(1)"),
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

/// Arrow functions (clj-quickjs-53) are closures: the same lowering as an
/// anonymous function expression (the subset has no `this` or `arguments`
/// for them to differ on). The first three scripts are the
/// `test_parse_arrow_function` cases of
/// `research/quickjs/tests/test_language.js` (QuickJS, revision
/// 7d1c13c5d8f8; see `docs/architecture.md` for the attribution), checked
/// here as completion values instead of through `eval` and `typeof`:
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
fn arrow_function_values() {
    // ASI after the arrow body: two statements, the second one's value.
    assert_eq!(num("() => {}\n+1"), 1.0);
    // The completion value is the second arrow, a function object: calling
    // it from another task returns what the second body returns.
    let rt = Runtime::new();
    let apply = compile_function("function apply(f) { return f(); }").unwrap();
    let call_completion = |src: &str| {
        let task = rt.spawn(&exe(src), &[]).unwrap();
        let RunOutcome::Complete(f) = task.run().unwrap() else {
            panic!("{src:?}")
        };
        assert_eq!(f.view(), ValueView::Object, "{src:?}");
        // Captures nothing, so it is shareable.
        rt.publish(&f).unwrap();
        let other = rt.spawn(&apply, &[&f]).unwrap();
        match other.run().unwrap() {
            RunOutcome::Complete(v) => v.view(),
            other => panic!("{src:?}: {other:?}"),
        }
    };
    assert_eq!(call_completion("() => {}\n() => {}"), ValueView::Undefined);
    assert_eq!(call_completion("x => {}\n() => {}"), ValueView::Undefined);
    assert_eq!(
        call_completion("() => { return 1; }\n() => { return 2; }"),
        ValueView::Number(2.0)
    );
    assert_eq!(call_completion("() => 3"), ValueView::Number(3.0));

    // Parameters, concise bodies and closure capture.
    assert_eq!(num("const add = (a, b) => a + b; add(2, 3)"), 5.0);
    assert_eq!(num("const k = x => () => x; k(7)()"), 7.0);
    assert_eq!(num("((a, b,) => b)(1, 2, 3)"), 2.0);
    assert_eq!(eval("((a, b) => b)(1)"), ValueView::Undefined);
    assert_eq!(eval("(() => {})()"), ValueView::Undefined);
    // Block bodies return, see and assign enclosing bindings.
    assert_eq!(
        num("let n = 0;
             const inc = () => { n = n + 1; return n; };
             inc();
             inc()"),
        2.0
    );
    assert_eq!(
        num("const fact = n => n <= 1 ? 1 : n * fact(n - 1); fact(5)"),
        120.0
    );
    // Each evaluation creates a new function object.
    assert_eq!(
        eval("const mk = () => () => 1; mk() === mk()"),
        ValueView::Bool(false)
    );
    // An arrow is a callback like any function value.
    assert_eq!(
        num("__rt.reduce((acc, x) => acc + x * 10, 1, __rt.vector(1, 2, 3))"),
        61.0
    );
    // The temporal dead zone applies to what an arrow captures.
    assert_eq!(
        failure_at("const f = () => later;\nf();\nconst later = 1;").0,
        VmErrorKind::ReferenceError
    );
    // `this` and `arguments` stay rejected inside arrows.
    assert_eq!(
        compile_err("const f = () => this;"),
        (SyntaxErrorKind::Unsupported, 1, 17)
    );
    assert_eq!(
        compile_err("const f = (a) => arguments;"),
        (SyntaxErrorKind::Unsupported, 1, 18)
    );
}

/// Default parameter values (clj-quickjs-100). Two cases are adapted from
/// `research/quickjs/tests/test_language.js`, revision 7d1c13c5d8f8:
/// the arrow `(a, b = 1, c) => ...` of `test_function_length` (line 445),
/// called instead of asking its `length` (clj-quickjs-101), and
/// `f = function f(a = f) { return a; }; f() === f` of
/// `test_argument_scope` (line 453), as a `const` binding a differently
/// named expression and as a declaration. Its `arguments`, `this`
/// (`f.call(123)`; a `new F()` stand-in runs here) and `eval` cases wait
/// for clj-quickjs-97, -102 and -109; see
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
fn default_parameters() {
    // A default runs when its argument is missing or `undefined`, and only
    // then (clj-quickjs-100).
    let f = "function f(a, b = 2) { return b; }\n";
    assert_eq!(num(&format!("{f}f(1)")), 2.0);
    assert_eq!(num(&format!("{f}f(1, undefined)")), 2.0);
    assert_eq!(eval(&format!("{f}f(1, null)")), ValueView::Null);
    assert_eq!(num(&format!("{f}f(1, 0)")), 0.0);
    // Defaults see earlier parameters and run left to right.
    assert_eq!(num("function f(a, b = a + 1) { return b; } f(1)"), 2.0);
    assert_eq!(
        num("let log = 0;
             function f(a = (log = log * 10 + 1), b = (log = log * 10 + 2)) {}
             f(); f(5); log"),
        122.0
    );
    // A later parameter (or the parameter itself) is in its temporal dead
    // zone: a ReferenceError at the reference, only if the default runs.
    let (kind, message, line, column) = failure_at("function f(a = b, b) {}\nf()");
    assert_eq!(
        (kind, message.as_str(), line, column),
        (
            VmErrorKind::ReferenceError,
            "cannot access 'b' before initialization",
            1,
            16
        )
    );
    assert_eq!(
        failure_at("function f(a = a) {}\nf()").0,
        VmErrorKind::ReferenceError
    );
    assert_eq!(num("function f(a = b, b) { return a; } f(1, 2)"), 1.0);
    // Defaults see the function's own name and the enclosing scopes.
    assert_eq!(
        eval("const g = function h(a = h) { return a; }; g() === g"),
        ValueView::Bool(true)
    );
    assert_eq!(
        eval("function f(a = f) { return a; } f() === f"),
        ValueView::Bool(true)
    );
    // ... but not the body's declarations, which have their own scope.
    assert_eq!(
        num("const x = 1; function f(a = () => x) { let x = 2; return a(); } f()"),
        1.0
    );
    assert_eq!(
        num("const x = 1; function f(a = () => x) { var x = 2; return a(); } f()"),
        1.0
    );
    // Closures in defaults capture the parameter bindings: they observe
    // later assignments by the body, and a closure that runs before its
    // parameter is initialized fails at run time.
    assert_eq!(
        num("function f(a, g = () => a) { a = 2; return g(); } f(1)"),
        2.0
    );
    assert_eq!(
        num("function f(g = () => b, b) { return g(); } f(undefined, 5)"),
        5.0
    );
    let (kind, _, line, column) = failure_at("function f(g = () => b, b = g()) {}\nf()");
    assert_eq!((kind, line, column), (VmErrorKind::ReferenceError, 1, 22));
    // A body `var` that repeats a parameter is a separate binding that
    // starts with the parameter's value; closures in defaults keep the
    // parameter.
    assert_eq!(
        num("function f(a = 1, g = () => a) {
               var a; const r = a; a = 3;
               return r * 100 + a * 10 + g();
             }
             f()"),
        131.0
    );
    // A body `var` that repeats no parameter starts `undefined`, even in a
    // local the parameter scope used for scratch.
    assert_eq!(
        eval("function f(g = () => b, b) { var x; return x; } f(undefined, 7)"),
        ValueView::Undefined
    );
    // A body function declaration may repeat a parameter, and wins.
    assert_eq!(
        eval("function f(a = 1) { function a() {} return typeof a === \"function\"; } f()"),
        ValueView::Bool(true)
    );
    // A body `let`/`const` may not.
    assert_eq!(
        compile_err("function f(a = 1) { let a; }"),
        (SyntaxErrorKind::Invalid, 1, 25)
    );
    assert_eq!(
        compile_err("function f(a, b = 1) { const a = 2; }"),
        (SyntaxErrorKind::Invalid, 1, 30)
    );
    // The `docs/grammar.md` example.
    assert_eq!(
        eval(
            "const x = 1;
             function f(a, b = a + 1, g = () => x) { var x = 2; return b * 10 + g(); }
             f(1) === 21 && f(1, undefined) === 21 && f(1, 5) === 51 && ((a = 1, c) => a + c)(undefined, 2) === 3;"
        ),
        ValueView::Bool(true)
    );
    // Arrow functions, with parameters after a default.
    assert_eq!(num("((a, b = 1, c) => a + c)(1, 2, 3)"), 4.0);
    assert_eq!(num("((a = 1) => a)()"), 1.0);
    assert_eq!(num("const k = (a, b = () => a) => b; k(9)()"), 9.0);
    // The entry function of `compile_function`.
    let rt = Runtime::new();
    let entry = compile_function("function f(a, b = a * 2) { return b; }").unwrap();
    let three = rt.root(Primitive::Number(3.0));
    let task = rt.spawn(&entry, &[&three]).unwrap();
    match task.run().unwrap() {
        RunOutcome::Complete(v) => assert_eq!(v.view(), ValueView::Number(6.0)),
        other => panic!("{other:?}"),
    }
    // `arguments` (clj-quickjs-97) is not in the subset yet, in defaults as
    // elsewhere. A default sees its function's `this` (clj-quickjs-116; the
    // receiver of a method call, `undefined` for a plain call); an arrow's
    // default still rejects it (lexical `this`, clj-quickjs-98).
    assert_eq!(
        compile_err("function f(a, b = arguments[0]) { return b; }"),
        (SyntaxErrorKind::Unsupported, 1, 19)
    );
    assert_eq!(
        num("let o = {v: 4, F: function (a = this) { return a.v; }}; o.F()"),
        4.0
    );
    assert_eq!(
        eval("function F(a = this) { return a; } F()"),
        ValueView::Undefined
    );
    // In a construct call (clj-quickjs-71) a default sees the new object.
    assert_eq!(
        eval("function F(a = this) { this.d = a; } const o = new F(); o.d === o"),
        ValueView::Bool(true)
    );
    assert_eq!(
        compile_err("let F = (a = this) => a;"),
        (SyntaxErrorKind::Unsupported, 1, 14)
    );
    // An object literal default with an escaped reserved-word key.
    assert_eq!(
        num("const f = (a = {\\u0069f: 1, b: 2}) => a.if + a.b; f()"),
        3.0
    );
    // A `"use strict"` directive in a function with a default.
    assert_eq!(
        compile_err("function f(a = 1) {\n  \"use strict\";\n}"),
        (SyntaxErrorKind::Invalid, 2, 3)
    );
}

#[test]
fn default_parameter_closures_follow_publication_rules() {
    let rt = Runtime::new();
    let apply = compile_function("function apply(f) { return f(); }").unwrap();
    let completion = |src: &str| {
        let task = rt.spawn(&exe(src), &[]).unwrap();
        let RunOutcome::Complete(v) = task.run().unwrap() else {
            panic!("{src:?}")
        };
        v
    };
    // A closure over an earlier, never-assigned parameter captures its
    // value: shareable, and callable from another task.
    let g = completion("const k = (a, b = () => a) => b; k(9)");
    rt.publish(&g).expect("a value capture is shareable");
    let other = rt.spawn(&apply, &[&g]).unwrap();
    let RunOutcome::Complete(v) = other.run().unwrap() else {
        panic!()
    };
    assert_eq!(v.view(), ValueView::Number(9.0));
    // A closure over a later parameter, or over an assigned one, holds a
    // task-owned cell.
    for src in [
        "function f(g = () => b, b) { return g; } f(undefined, 1)",
        "function f(a, g = () => a) { a = 2; return g; } f(1)",
    ] {
        let g = completion(src);
        let err = rt.publish(&g).unwrap_err();
        assert!(
            matches!(
                err,
                PublishError::NotShareable {
                    found: "binding cell",
                    ..
                }
            ),
            "{src}: {err:?}"
        );
    }
}

#[test]
fn default_parameters_resume_across_yields_collection_and_threads() {
    // One instruction per run, each run on a new thread, a collection at
    // every yield: the uninitialized parameter cell, the argument moved to
    // a scratch local and the effect in a default all survive, and the
    // effect runs exactly once.
    let src = "function f(v, g = () => w, n = __rt.effect(7, __rt.count(v)), w = __rt.conj(v, n)) {
                 return __rt.count(g()) * 10000 + __rt.nth(g(), __rt.count(g()) - 1);
               }
               f(__rt.vector(1, 2)) + f(__rt.vector(1), undefined, 5, __rt.vector(3, 4, 9)) * 100000";
    let rt = Runtime::new();
    let task = rt.spawn(&exe(src), &[]).unwrap();
    let mut host = PlusThousand::default();
    let mut yields = 0;
    let result = loop {
        let outcome = std::thread::scope(|s| {
            s.spawn(|| task.run_with(Budget::instructions(1), &mut host))
                .join()
                .unwrap()
        })
        .unwrap();
        match outcome {
            RunOutcome::Yielded => {
                yields += 1;
                rt.collect().unwrap();
            }
            RunOutcome::Complete(v) => break v.view(),
            other => panic!("{other:?}"),
        }
    };
    // 3 elements ending in 1002, then 3 elements ending in 9.
    assert_eq!(result, ValueView::Number(3000931002.0));
    assert_eq!(host.calls, [2.0]);
    assert!(yields > 50, "only {yields} yields");
}

/// `Function.prototype.call` / `apply` and `Reflect.apply`
/// (clj-quickjs-102). The first four rows are adapted from `test_function`
/// (line 83) of `research/quickjs/tests/test_builtin.js`, revision
/// 7d1c13c5d8f8, with its helper `my_func` (line 78); `assert_throws` is
/// a `try`/`catch` here. See `docs/architecture.md`, "QuickJS references
/// and provenance".
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
fn call_apply_and_reflect_apply() {
    let truth = |src: &str| assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    // test_builtin.js `test_function`.
    let my_func = "function my_func(a, b) { return a + b; } ";
    truth(&format!("{my_func} my_func.call(null, 1, 2) === 3"));
    truth(&format!("{my_func} my_func.apply(null, [1, 2]) === 3"));
    truth("(function () { return 1; }).apply(null, undefined) === 1");
    truth(
        "let r = 0; try { Reflect.apply(function () { return 1; }, null, undefined); } \
         catch (e) { r = 1; } r === 1",
    );

    // `this` of a strict callee is the receiver passed, unchanged.
    let f = "function f() { return this; } ";
    assert_eq!(num(&format!("{f} f.call(7)")), 7.0);
    assert_eq!(num(&format!("{f} f.apply(8)")), 8.0);
    assert_eq!(eval(&format!("{f} f.call()")), ValueView::Undefined);
    assert_eq!(num(&format!("{f} Reflect.apply(f, 9, [])")), 9.0);
    // Arguments, from an array, an array-like object (holes read through
    // the prototype chain) and `call`'s rest; extra ones are ignored.
    let g = "function g(a, b, c) { return this * 1000 + a * 100 + b * 10 + c; } ";
    assert_eq!(num(&format!("{g} g.call(1, 2, 3, 4, 5)")), 1234.0);
    assert_eq!(num(&format!("{g} g.apply(1, [2, 3, 4])")), 1234.0);
    assert_eq!(
        num(&format!(
            "{g} g.apply(1, {{length: 3, 0: 2, 1: 3, 2: 4, 3: 9}})"
        )),
        1234.0
    );
    assert_eq!(
        num(&format!(
            "{g} let p = {{2: 4}}; let o = Object.create(p); o[0] = 2; o[1] = 3; o.length = '3'; \
             Reflect.apply(g, 1, o)"
        )),
        1234.0
    );
    // `length` 2.9 is 2 arguments; a missing one is `undefined`.
    truth(
        "Reflect.apply(function (a, b, c) { return c === undefined && a + b === 5; }, 1, \
         {length: 2.9, 0: 2, 1: 3, 2: 4})",
    );
    // `call` and `apply` through each other, and through `Reflect.apply`.
    assert_eq!(num(&format!("{g} g.call.call(g, 1, 2, 3, 4)")), 1234.0);
    assert_eq!(num(&format!("{g} g.apply.call(g, 1, [2, 3, 4])")), 1234.0);
    assert_eq!(
        num(&format!(
            "{g} Reflect.apply(Function.prototype.call, g, [1, 2, 3, 4])"
        )),
        1234.0
    );
    // A built-in target runs to completion.
    truth("Object.is.call(null, 1, 1) && !Reflect.apply(Object.is, null, [1, 2])");
    truth("Object.prototype.toString.call([]) === '[object Array]'");
    // Function.prototype is the prototype of every function, is itself
    // callable and has `call` and `apply`.
    truth(
        "function h() {} Object.getPrototypeOf(h) === Function.prototype && \
         Object.getPrototypeOf(Object.is) === Function.prototype && \
         Object.getPrototypeOf(Function.prototype) === Object.prototype && \
         Function.prototype() === undefined && typeof Function.prototype === 'function' && \
         h.call === Function.prototype.call && Object.is.apply === h.apply && \
         Function.prototype.call.length === 1 && Function.prototype.apply.length === 2 && \
         Reflect.apply.length === 3 && Function.prototype.call.name === 'call' && \
         h instanceof Function && h instanceof Object && Object.is instanceof Function && \
         h.constructor === Function && Function.prototype.constructor === Function",
    );

    // The `docs/grammar.md` example lines, each true.
    for src in [
        "function add(a, b) { return a + b; } add.call(null, 1, 2) === 3 && add.apply(null, [1, 2]) === 3 && Reflect.apply(add, null, [1, 2]) === 3;",
        "function self() { return this; } self.call(7) === 7 && self.apply(8) === 8 && self.call() === undefined && Object.getPrototypeOf(self) === Function.prototype;",
        "let r = false; try { Reflect.apply(function () {}, null, undefined); } catch (e) { r = true; } r;",
    ] {
        truth(src);
    }

    // Failures: catchable TypeErrors.
    for src in [
        "Reflect.apply(f, null, undefined)",
        "f.apply(null, 1)",
        "Function.prototype.call.call(1)",
        "Function.prototype.apply.call({}, null, [])",
        "Reflect.apply(1, null, [])",
        "const c = f.call; c(1)",
    ] {
        assert_eq!(
            num(&format!(
                "{f} let r = 0; try {{ {src}; }} catch (e) {{ r = 1; }} r"
            )),
            1.0,
            "{src}"
        );
        assert_eq!(
            failure(&format!("{f} {src};")).kind(),
            VmErrorKind::TypeError
        );
    }
    // A forwarding cycle through an array holding itself ends as a
    // (catchable) stack overflow instead of looping inside one instruction.
    assert_eq!(
        failure(
            "const ap = Function.prototype.apply; const a = [ap, 0]; a[1] = a; ap.apply(ap, a);"
        )
        .kind(),
        VmErrorKind::StackOverflow
    );
    // A forwarding built-in as a native's callback runs with `this`
    // undefined, like any callback: `call` of `undefined` is a TypeError.
    assert_eq!(
        failure("__rt.reduce(Function.prototype.call, 0, __rt.vector(1));").kind(),
        VmErrorKind::TypeError
    );
    // The Function constructor is a stub (clj-quickjs-111).
    assert_eq!(
        failure("Function('return 1');").kind(),
        VmErrorKind::UnsupportedOperand
    );
    assert_eq!(
        failure("new Function('return 1');").kind(),
        VmErrorKind::TypeError
    );
    // A sloppy callee's `this` substitution (global object, wrappers) is a
    // documented gap: the explicit failure of `this` in sloppy code.
    let sloppy = compile_script_with(
        "sloppy",
        "function s() { return this; } s.call(7);",
        ScriptOptions::SLOPPY,
    )
    .unwrap();
    let rt = Runtime::new();
    let task = rt.spawn(&sloppy, &[]).unwrap();
    match task.run().unwrap() {
        RunOutcome::Failed(e) => assert_eq!(e.kind(), VmErrorKind::UnsupportedOperand),
        other => panic!("{other:?}"),
    }

    // A callee reached through `.call` that loops runs to completion under
    // a tiny budget, yielding and collecting between slices, each slice on
    // a new thread: it runs in a task-owned VM frame, not a Rust frame.
    let src = "\
               function loop(n) { let s = 0; for (let i = 0; i < n; i++) { s = s + this.k + i; } return s; } \
               const o = {k: 1}; \
               loop.call(o, 40) + loop.apply(o, [10]) * 10000 + Reflect.apply(loop, o, [5]) * 100000000";
    let rt = Runtime::new();
    let task = rt.spawn(&exe(src), &[]).unwrap();
    let mut yields = 0;
    let result = loop {
        let outcome = std::thread::scope(|s| {
            s.spawn(|| task.run_with(Budget::instructions(5), &mut PlusThousand::default()))
                .join()
                .unwrap()
        })
        .unwrap();
        match outcome {
            RunOutcome::Yielded => {
                yields += 1;
                rt.collect().unwrap();
            }
            RunOutcome::Complete(v) => break v.view(),
            other => panic!("{other:?}"),
        }
    };
    // 40 + 780, 10 + 45, 5 + 10.
    assert_eq!(
        result,
        ValueView::Number(820.0 + 550_000.0 + 1_500_000_000.0)
    );
    assert!(yields > 50, "only {yields} yields");
}

/// `apply` and `Reflect.apply` with a `null` list or an array-like whose
/// `length` is missing, negative or NaN call with no arguments
/// (clj-quickjs-102, CreateListFromArrayLike / ToLength); `Reflect.apply`
/// with a `null` list is a catchable TypeError.
#[test]
fn apply_null_lists_and_zero_lengths() {
    let none = "function none(a) { return a === undefined; } ";
    for list in [
        "null",
        "{0: 9}",
        "{length: -1, 0: 9}",
        "{length: NaN, 0: 9}",
        "{length: 'x', 0: 9}",
        "{length: undefined, 0: 9}",
    ] {
        let src =
            format!("{none} none.apply(null, {list}) && Reflect.apply(none, null, {list} || {{}})");
        assert_eq!(eval(&src), ValueView::Bool(true), "{src}");
        if list != "null" {
            let src = format!("{none} Reflect.apply(none, null, {list})");
            assert_eq!(eval(&src), ValueView::Bool(true), "{src}");
        }
    }
    assert_eq!(
        num(&format!(
            "{none} let r = 0; try {{ Reflect.apply(none, null, null); }} catch (e) {{ r = 1; }} r"
        )),
        1.0
    );
    assert_eq!(
        failure(&format!("{none} Reflect.apply(none, null, null);")).kind(),
        VmErrorKind::TypeError
    );
}

/// The forwarded call replaces the forwarding call's operands, so its
/// argument room is counted from the replacement base, not from the top
/// of the forwarding call's operands (clj-quickjs-102): a two-element
/// list fits a value-stack limit of five, the frame's own operand peak,
/// and a list longer than the room left is a catchable `StackOverflow`.
#[test]
fn forwarded_calls_reuse_operand_capacity() {
    let run = |src: &str, limit: usize| {
        let rt = Runtime::with_limits(VmLimits {
            max_call_depth: 100,
            max_stack_values: limit,
        });
        let exe = compile_function(src).unwrap_or_else(|e| panic!("{src}: {e}"));
        rt.spawn(&exe, &[]).unwrap().run().unwrap()
    };
    for src in [
        "function main() { return Object.is.apply(null, [1, 1]); }",
        "function main() { return Reflect.apply(Object.is, null, [1, 1]); }",
    ] {
        match run(src, 5) {
            RunOutcome::Complete(v) => assert_eq!(v.view(), ValueView::Bool(true), "{src}"),
            other => panic!("{src}: {other:?}"),
        }
    }
    let long = "function main() { return Object.is.apply(null, [1, 1, 1, 1, 1, 1, 1, 1]); }";
    match run(long, 8) {
        RunOutcome::Failed(e) => assert_eq!(e.kind(), VmErrorKind::StackOverflow),
        other => panic!("{other:?}"),
    }
    match run(long, 9) {
        RunOutcome::Complete(v) => assert_eq!(v.view(), ValueView::Bool(true)),
        other => panic!("{other:?}"),
    }
}

/// A callee reached through `call`, `apply` and `Reflect.apply`
/// (clj-quickjs-102) waits for a dependency inside the forwarded frame: the
/// task suspends with no Rust frame for the forwarding built-ins, survives
/// a collection, and resumes on another thread exactly at the `need`.
#[test]
fn forwarded_calls_wait_and_resume_on_another_thread() {
    let src = "function get(k) { return __rt.need(5, 3) * this.m + k; } \
               const o = {m: 10}; \
               get.call(o, 1) + Reflect.apply(get, o, [2]) * 1000 + get.apply(o, [3]) * 1000000";
    let rt = Runtime::new();
    let mut task = rt.spawn(&exe(src), &[]).unwrap();
    let expected = DependencyKey::new(rt.world(), DependencyId::new(5), ResultKind::new(3));
    let mut waits = 0;
    let result = loop {
        let outcome = std::thread::scope(|s| {
            s.spawn(|| task.run_with(Budget::UNLIMITED, &mut PlusThousand::default()))
                .join()
                .unwrap()
        })
        .unwrap();
        match outcome {
            RunOutcome::Waiting(key) => {
                assert_eq!(key, expected);
                waits += 1;
                rt.collect().unwrap();
                let mut requests = task.take_producer_requests();
                if let Some(request) = requests.pop() {
                    let value = rt.root(Primitive::Number(4.0));
                    let wakeups = rt
                        .complete(request.key(), request.producer(), Completion::Ready(&value))
                        .unwrap();
                    let mut runnable = wakeups.into_runnable();
                    assert_eq!(runnable.len(), 1);
                    task = runnable.pop().unwrap();
                }
            }
            RunOutcome::Complete(v) => break v.view(),
            other => panic!("{other:?}"),
        }
    };
    // The dependency is ready after the first wait: 41, 42, 43.
    assert_eq!(waits, 1);
    assert_eq!(result, ValueView::Number(41.0 + 42_000.0 + 43_000_000.0));
}
