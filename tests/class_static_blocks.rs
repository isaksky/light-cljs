//! Synthetic JavaScript fixtures, not light-cljs output.

use clj_quickjs::frontend::compile_script;
use clj_quickjs::{
    Budget, Completion, Executable, Host, HostCall, Primitive, RunOutcome, Runtime, ValueView,
};

struct NoEffects;
impl Host for NoEffects {
    fn effect(&mut self, _: HostCall) -> Result<Primitive, String> {
        panic!("unexpected effect")
    }
}

fn check(source: &str) {
    let code = compile_script("static-block.js", source).unwrap();
    let code = Executable::load(code.to_spec()).unwrap();
    let rt = Runtime::new();
    let result = rt.spawn(&code, &[]).unwrap().run().unwrap();
    assert!(
        matches!(result, RunOutcome::Complete(ref v) if v.view() == ValueView::Bool(true)),
        "{source}: {result:?}"
    );
}

#[test]
fn class_static_blocks_execute_in_order() {
    for source in [
        "class A { static { A.y = 2; } } A.y === 2",
        "let log = ''; class A { static a = (log += 'a'); static { log += 'b'; this.x = this.f(); } static c = (log += 'c'); static { log += 'd'; } static f() { return 7; } } new A(); new A(); log === 'abcd' && A.x === 7",
        "let log = ''; class A { static { log += 'b'; } static [(log += 'k', 'x')] = (log += 'f'); [(log += 'j', 'm')]() {} } log === 'kjbf'",
        "const C = class Inner { static { this.self = Inner; } }; C.self === C",
        "let n = 0; for (let i = 0; i < 3; i++) { class C { static { n++; } } } n === 3",
        "class C { static x = 1\nstatic { this.x++; } static y = this.x; } C.y === 2",
        "class C { static {} static() { return 1; } static x = 2; } new C().static() === 1 && C.x === 2 && Object.getOwnPropertyNames(C).join(',') === 'length,name,prototype,x'",
        "let escaped; let log = ''; try { class C { static { escaped = this; log += 'a'; throw 17; } static x = (log += 'x'); static { log += 'b'; } } } catch(e) { log += e; } log === 'a17' && escaped.x === undefined",
        "let caught = false; try { class C { static { C = 1; } } } catch(e) { caught = e instanceof TypeError; } caught",
        "let caught = false; try { let C = class { static { C; } }; } catch(e) { caught = e instanceof ReferenceError; } caught",
    ] {
        check(source);
    }
}

#[test]
fn class_static_blocks_scope_and_class_context() {
    for source in [
        "let x = 9; class C { static { this.before = x; var x = 1; this.f = () => ++x; } static { var x = 3; this.other = () => x; } static y = x; } C.before === undefined && C.f() === 2 && C.f() === 3 && C.other() === 3 && C.y === 9 && x === 9",
        "class C { static { this.x = f(); function f() { return 7; } } static { this.y = typeof f; } } C.x === 7 && C.y === 'undefined'",
        "class B { static get x() { return this.y; } static f() { return this.y + 1; } } class C extends B { static y = 7; static { this.a = super.x; this.b = (() => super.f())(); this.c = eval('super.f()'); super.z = 9; } } C.a === 7 && C.b === 8 && C.c === 8 && C.z === 9 && B.z === undefined",
        "class C { static { this.a = this.#f(); this.get = () => this.#x; } static #x = 7; static #f() { return 8; } } C.a === 8 && C.get() === 7",
        "let caught = false; class C { static { try { this.#x; } catch(e) { caught = e instanceof TypeError; } } static #x = 7; } caught",
        "class C { static { this.a = this; this.b = () => this; this.c = new.target; this.d = eval('new.target'); } } C.a === C && C.b() === C && C.c === undefined && C.d === undefined",
        "class C { static { this.x = eval('var v = 7; v'); this.y = typeof v; } } C.x === 7 && C.y === 'undefined'",
        "class C { static { this.x = (function(x) { return arguments[0]; })(7); this.y = (() => { return 8; })(); } } C.x === 7 && C.y === 8",
        "function f() { class C { static {} } return arguments[0]; } f(7) === 7",
        "class C { static { this.x = {arguments: 7}.arguments; } } C.x === 7",
        "class C { static {} \\u0069f = 7; } new C().if === 7",
        "class C { static { class Inner { static { this.x = 7; } } this.x = Inner.x; } } C.x === 7",
        "class C { static { this.f = async () => await 7; this.g = function*() { yield 8; }; } } C.g().next().value === 8",
        "class C { static { let caught = false; try { eval('arguments'); } catch(e) { caught = e instanceof SyntaxError; } this.ok = caught; } } C.ok",
        "class C { static { let caught = false; try { eval('eval(\"arguments\")'); } catch(e) { caught = e instanceof SyntaxError; } this.ok = caught; } } C.ok",
        "class C { static { for (let i = 0; i < 2; i++) { if (i === 0) continue; break; } this.ok = true; } } C.ok",
    ] {
        check(source);
    }
}

#[test]
fn class_static_blocks_restore_enclosing_context() {
    for source in [
        "function* f() { class C { static {} } yield 7; } f().next().value === 7",
        "class B { constructor() { this.x = 7; } } class D extends B { y = 8; constructor() { class C { static {} } super(); } } const d = new D(); d.x === 7 && d.y === 8",
        "class B { f() { return 7; } } class D extends B { f() { class C { static {} } return super.f(); } } new D().f() === 7",
        "let n = 0; outer: while (n < 2) { n++; class C { static {} } continue outer; } n === 2",
        "let n = 0; switch (7) { case 7: class C { static {} } n++; break; default: n = 9; } n === 1",
    ] {
        check(source);
    }
    compile_script(
        "static-block-async-context.js",
        "async function f() { class C { static {} } return await 7; }",
    )
    .unwrap();
}

#[test]
fn class_static_blocks_early_errors() {
    for source in [
        "class C { static { return; } }",
        "function f() { class C { static { return 1; } } }",
        "class C { static { arguments; } }",
        "class C { static { () => arguments; } }",
        "class C { static { ({arguments}); } }",
        "class C { static { await 1; } }",
        "async function f() { class C { static { await 1; } } }",
        "class C { static { let await; } }",
        "class C { static { super(); } }",
        "class B {} class C extends B { constructor() { class D { static { super(); } } } }",
        "while (false) { class C { static { break; } } }",
        "outer: while (false) { class C { static { break outer; } } }",
        "class C { static { let x; var x; } }",
    ] {
        assert!(
            compile_script("static-block-error.js", source).is_err(),
            "{source}"
        );
    }
}

#[test]
fn class_static_blocks_wait_collect_and_migrate() {
    let source = "let log = ''; class B { static f() { return __rt.need(292, 1); } } class C extends B { static #x = 3; static { var x = {n: this.#x}; log += 'a'; this.answer = x.n + eval('super.f()'); this.read = () => x.n + this.#x; log += 'b'; } static y = (log += 'c'); } log === 'abc' && C.answer === 10 && C.read() === 6";
    let code = compile_script("static-block-wait.js", source).unwrap();
    let code = Executable::load(code.to_spec()).unwrap();
    let rt = Runtime::new();
    // Initialize the shared builtins on the host stack, as for other small-worker tests.
    rt.spawn(&compile_script("warmup.js", "Object").unwrap(), &[])
        .unwrap()
        .run()
        .unwrap();
    let mut task = rt.spawn(&code, &[]).unwrap();
    let mut waits = 0;
    for _ in 0..2000 {
        let (next, outcome) = std::thread::Builder::new()
            .stack_size(64 * 1024)
            .spawn(move || {
                let result = task
                    .run_with(Budget::instructions(5), &mut NoEffects)
                    .unwrap();
                (task, result)
            })
            .unwrap()
            .join()
            .unwrap();
        task = next;
        rt.collect().unwrap();
        match outcome {
            RunOutcome::Yielded => {}
            RunOutcome::Waiting(_) => {
                waits += 1;
                assert_eq!(waits, 1);
                let request = task.take_producer_requests().pop().unwrap();
                let value = rt.root(Primitive::Number(7.0));
                task = rt
                    .complete(request.key(), request.producer(), Completion::Ready(&value))
                    .unwrap()
                    .into_runnable()
                    .pop()
                    .unwrap();
            }
            RunOutcome::Complete(value) => {
                assert_eq!(value.view(), ValueView::Bool(true));
                assert_eq!(waits, 1);
                return;
            }
            other => panic!("{other:?}"),
        }
    }
    panic!("static block did not finish");
}
