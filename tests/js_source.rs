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

/// Synthetic fixtures, not light-cljs output.
#[test]
fn array_destructuring() {
    for source in [
        "let [a, , b = 3, ...r] = [1, 2, undefined, 4, 5]; a === 1 && b === 3 && r.length === 2 && r[0] === 4",
        "let p = 1, q = 2; [p, q] = [q, p]; p === 2 && q === 1",
        "let it = {next() { return {done: true}; }}; it[Symbol.iterator] = function() { return this; }; let [x] = it; x === undefined",
        "let n = 0; let [x = ++n] = [2]; x === 2 && n === 0",
        "let closed = 0; let it = {next() { return {value: 1}; }, return() { closed++; return {}; }}; it[Symbol.iterator] = function() { return this; }; let [x] = it; x === 1 && closed === 1",
        "let closed = 0; let it = {next() { return {done: true}; }, return() { closed++; return {}; }}; it[Symbol.iterator] = function() { return this; }; let [x, y] = it; closed === 0 && x === undefined && y === undefined",
        "let n = 0; try { let [z] = 1; } catch(e) { if (e instanceof TypeError) n++; } try { let [z] = null; } catch(e) { if (e instanceof TypeError) n++; } n === 2",
        "(([a, b]) => a + b)([1, 2]) === 3",
        "try { throw [7]; } catch ([e]) { e === 7; }",
        "let [[d]] = [[1]]; d === 1",
        "let o = {}, a = []; [o.x, a[0]] = [3, 4]; o.x === 3 && a[0] === 4",
        "var [a, ...r] = [1, 2]; a === 1 && r[0] === 2",
        "function f([a, b] = [2, 3]) { return a + b; } f() === 5 && f.length === 0 && (([a,b]) => 0).length === 1",
        "function* values([a, b] = [3, 4]) { yield a; yield b; return 9; } let [x, ...r] = values(); x === 3 && r.length === 1 && r[0] === 4",
        "function* finished([a] = [7]) { return a; } let [x] = finished(); x === undefined",
    ] {
        assert_eq!(eval(source), ValueView::Bool(true), "{source}");
    }
}

/// Synthetic fixtures, not light-cljs output.
#[test]
fn array_destructuring_protocol_and_abrupt_completion() {
    for source in [
        "let n = 0; let it = {next() { n++; return {done: true, get value() { throw 99; }}; }, return() { throw 98; }}; it[Symbol.iterator] = function() { return this; }; let [a, b, ...r] = it; n === 1 && a === undefined && b === undefined && r.length === 0",
        "let n = 0; let it = {next() { n++; return {get value() { throw 99; }}; }, return() { n += 10; return {}; }}; it[Symbol.iterator] = function() { return this; }; let [,] = it; n === 11",
        "let n = 0; let it = {next() { n++; return {value: n}; }, return() { n += 10; return {}; }}; it[Symbol.iterator] = function() { return this; }; let [] = it; n === 10",
        "let log = ''; let it = {next() { log += 'n'; return {value: undefined}; }, return() { log += 'r'; throw 99; }}; it[Symbol.iterator] = function() { return this; }; function fail() { throw 17; } let caught; try { let [x = fail()] = it; } catch(e) { caught = e; } caught === 17 && log === 'nr'",
        "let log = ''; let o = {set x(v) { log += 's'; throw 17; }}; let it = {next() { log += 'n'; return {value: 1}; }, return() { log += 'r'; return {}; }}; it[Symbol.iterator] = function() { return this; }; let caught; try { [o.x] = it; } catch(e) { caught = e; } caught === 17 && log === 'nsr'",
        "let log = ''; let o = {}; function key() { log += 'k'; return 'x'; } let it = {next() { log += 'n'; return {value: undefined}; }, return() { log += 'r'; return {}; }}; it[Symbol.iterator] = function() { return this; }; function def() { log += 'd'; return 3; } [o[key()] = def()] = it; o.x === 3 && log === 'kndr'",
        "let n = 0; let it = {next() { throw 17; }, return() { n++; return {}; }}; it[Symbol.iterator] = function() { return this; }; let error; try { let [x] = it; } catch(e) { error = e; } error === 17 && n === 0",
        "let n = 0; let it = {next() { return {get value() { throw 17; }}; }, return() { n++; return {}; }}; it[Symbol.iterator] = function() { return this; }; let error; try { let [x] = it; } catch(e) { error = e; } error === 17 && n === 0",
        "let n = 0; let it = {next() { return {value: 1}; }, return() { n++; return 1; }}; it[Symbol.iterator] = function() { return this; }; let error; try { let [x] = it; } catch(e) { error = e instanceof TypeError; } error && n === 1",
        "let n = 0; let it = {next() { return {value: 1}; }, return() { n++; throw 17; }}; it[Symbol.iterator] = function() { return this; }; let error; try { let [x] = it; } catch(e) { error = e; } error === 17 && n === 1",
        "let n = 0; function f([a = () => b, b = 2]) { return a(); } f([]) === 2",
        "let caught = false; try { let [a = b, b] = []; } catch(e) { caught = e instanceof ReferenceError; } caught",
        "let [f = function() {}] = []; f.name === 'f'",
        "let a = [1, 2]; let x; let v = ([x] = a); v === a && x === 1",
        "let a; [a = /abc/] = []; a.test('abc')",
        "let a; [...a /* , allowed in a comment */] = [1]; a[0] === 1",
        "let a; [[a] = [3]] = []; a === 3",
        "let n = 0; for (let [a] = [2]; n < a; n++) {} n === 2",
        "function f([a], c = a) { var a = 5; return c + a; } f([2]) === 7",
        "let f; try { throw []; } catch([a = 3]) { f = () => a; } f() === 3",
        "let log = ''; let key = {toString() { log += 'k'; return 'x'; }}; let o = {set x(v) { log += 's'; }}; let it = {next() { log += 'n'; return {}; }, return() { log += 'r'; return {}; }}; it[Symbol.iterator] = function() { return this; }; function def() { log += 'd'; return 7; } [o[key] = def()] = it; log === 'ndksr'",
        "let log = ''; let it = {next() { log += 'n'; return {}; }, return() { log += 'r'; return {}; }}; it[Symbol.iterator] = function() { return this; }; function def() { log += 'd'; return 7; } let caught = false; try { [null['x'] = def()] = it; } catch(e) { caught = e instanceof TypeError; } caught && log === 'ndr'",
        "let caught = 0; try { let [x] = undefined; } catch(e) { if (e instanceof TypeError) caught++; } caught === 1",
        "let [a, ...r] = 'a\\u{1f600}'; a === 'a' && r.length === 1 && r[0].length === 2",
        "((c, [a, b] = 1, d) => 0).length === 1",
    ] {
        assert_eq!(eval(source), ValueView::Bool(true), "{source}");
    }
}

/// Synthetic fixtures, not light-cljs output.
#[test]
fn array_destructuring_nested_iterator_close() {
    for (action, inner_close, outer_close, expected) in [
        (
            "let [[x]] = outer;",
            "return {};",
            "return {};",
            "caught === undefined",
        ),
        (
            "let [[x = fail()]] = outer;",
            "return {};",
            "return {};",
            "caught === 17",
        ),
        (
            "let [[x = fail()]] = outer;",
            "throw 18;",
            "throw 19;",
            "caught === 17",
        ),
        (
            "let [[x]] = outer;",
            "throw 18;",
            "throw 19;",
            "caught === 18",
        ),
    ] {
        let source = format!(
            "let log = ''; let inner = {{next() {{ return {{}}; }}, return() {{ log += 'i'; {inner_close} }} }}; inner[Symbol.iterator] = function() {{ return this; }}; let outer = {{next() {{ return {{value: inner}}; }}, return() {{ log += 'o'; {outer_close} }} }}; outer[Symbol.iterator] = function() {{ return this; }}; function fail() {{ throw 17; }} let caught; try {{ {action} }} catch(e) {{ caught = e; }} log === 'io' && {expected}"
        );
        assert_eq!(eval(&source), ValueView::Bool(true), "{source}");
    }
}

/// Synthetic fixtures, not light-cljs output. Collect after every instruction,
/// satisfy one dependency wait, and move the continuation to a new worker.
#[test]
fn array_destructuring_waits_collects_and_migrates() {
    for source in [
        "let n = 0; let it = {next() { n++; __rt.need(131, 1); return {value: [7]}; }, return() { n += 10; return {}; }}; it[Symbol.iterator] = function() { return this; }; let [[a]] = it; a === 7 && n === 11",
        "let it = {next() { return {done: false, get value() { __rt.need(131, 1); return [7]; }}; }}; it[Symbol.iterator] = function() { return this; }; let [[a]] = it; a === 7",
        "let it = {next() { return {get done() { __rt.need(131, 1); return true; }}; }, return() { throw 99; }}; it[Symbol.iterator] = function() { return this; }; let [a = 7] = it; a === 7",
        "let n = 0; let it = {next() { return {value: 7}; }, get return() { __rt.need(131, 1); return function() { n++; return {}; }; }}; it[Symbol.iterator] = function() { return this; }; let [a] = it; a === 7 && n === 1",
        "let n = 0; let it = {next() { return {value: 7}; }, return() { n++; __rt.need(131, 1); return {}; }}; it[Symbol.iterator] = function() { return this; }; let [a] = it; a === 7 && n === 1",
        "let key = {toString() { __rt.need(131, 1); return 'x'; }}; let o = {}; [o[key]] = [7]; o.x === 7",
        "let n = 0; let o = {set x(v) { __rt.need(131, 1); n = v; }}; [o.x] = [7]; n === 7",
        "function f([a = __rt.need(131, 1)]) { return () => a; } f([])() === 7",
        "function* values([a = __rt.need(131, 1)]) { yield a; } let [...r] = values([]); r.length === 1 && r[0] === 7",
        "let n = 0; let it = {next() { return {value: undefined}; }, return() { n++; __rt.need(131, 1); throw 99; }}; it[Symbol.iterator] = function() { return this; }; function fail() { throw 17; } let error; try { let [a = fail()] = it; } catch(e) { error = e; } error === 17 && n === 1",
    ] {
        let rt = Runtime::new();
        let mut task = rt.spawn(&exe(source), &[]).unwrap();
        let mut waits = 0;
        let mut complete = false;
        for _ in 0..1500 {
            let (next, outcome) = std::thread::spawn(move || {
                let outcome = task
                    .run_with(Budget::instructions(1), &mut NoEffects)
                    .unwrap();
                (task, outcome)
            })
            .join()
            .unwrap();
            task = next;
            rt.collect().unwrap();
            match outcome {
                RunOutcome::Yielded => {}
                RunOutcome::Waiting(_) => {
                    waits += 1;
                    assert_eq!(waits, 1, "{source}");
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
                    assert_eq!(value.view(), ValueView::Bool(true), "{source}");
                    complete = true;
                    break;
                }
                other => panic!("{source}: {other:?}"),
            }
        }
        assert!(complete, "instruction bound exceeded: {source}");
        assert_eq!(waits, 1, "{source}");
    }
}

// First two rows adapt test_spread in research/quickjs/tests/test_language.js,
// revision 7d1c13c5d8f8; synthetic fixtures, not light-cljs output.
// Copyright (c) 2017-2021 Fabrice Bellard
// Copyright (c) 2017-2021 Charlie Gordon
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL
// THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
// THE SOFTWARE.
#[test]
fn array_spread_and_spread_arguments() {
    for source in [
        "let x = [1, 2, ...[3, 4]]; x.length === 4 && x[0] === 1 && x[1] === 2 && x[2] === 3 && x[3] === 4",
        "let x = [...[,]]; let names = Object.getOwnPropertyNames(x); x[0] === undefined && names.length === 2 && names[0] === '0' && names[1] === 'length'",
        "function f(a, b, c) { return a + b * 10 + c * 100; } f(...[1, 2], 3) === 321",
        "let o = {m(a) { return this === o && a === 1; }}; o.m(...[1]) && o['m'](...[1])",
        "let n = 0; let it = {}; it[Symbol.iterator] = function() { return {next() { n++; return {done: n > 3, value: n}; }}; }; let x = [0, ...it, n]; n === 4 && x.length === 5 && x[1] === 1 && x[3] === 3 && x[4] === 4",
        "let n = 0; try { [...1]; } catch(e) { if (e instanceof TypeError) n++; } try { (function() {})(...null); } catch(e) { if (e instanceof TypeError) n++; } n === 2",
        "function F(a, b) { this.n = a + b; } new F(...[2, 3]).n === 5",
        "let it = [1].values(); Array.prototype[Symbol.iterator] === Array.prototype.values && it[Symbol.iterator]() === it",
    ] {
        assert_eq!(eval(source), ValueView::Bool(true), "{source}");
    }
}

/// Synthetic fixtures, not light-cljs output.
#[test]
fn spread_protocol_errors_and_order() {
    for source in [
        "let a = [1]; let it = a.values(); a.push(2); let x = [...it]; x.length === 2 && x[1] === 2 && [...it].length === 0",
        "let a = [,]; Object.setPrototypeOf(a, {0: 5}); [...Array.prototype.values.call(a)][0] === 5",
        "function f(a, b, c) { return a + b * 10 + c * 100; } f(...[], ...[1], ...[2, 3]) === 321 && [...[], 1, ...[], 2].length === 2",
        "let n = 0; let it = {next() { n++; this.next = function() { throw 99; }; return {get done() { return n > 2; }, get value() { return n; }}; }}; it[Symbol.iterator] = function() { return this; }; let a = [...it]; n === 3 && a[0] === 1 && a[1] === 2",
        "let it = {next() { return {done: true, get value() { throw 99; }}; }}; it[Symbol.iterator] = function() { return this; }; [...it].length === 0",
        "let a = [1]; a[Symbol.iterator] = function() { return [7].values(); }; [...a][0] === 7 && [...'A\\u{1f600}'].length === 2 && [...new Uint8Array([2,3])][1] === 3",
        "let log = ''; function v(n) { log += n; return n; } function f() { return arguments[0] + arguments[1] + arguments[2]; } let it = {next() { log += 'n'; return {done: log.length > 2, value: 2}; }}; it[Symbol.iterator] = function() { log += 'i'; return this; }; f(v(1), ...it, v(3)); log === '1in3'",
        "let n = 0; let it = {}; it[Symbol.iterator] = 1; try { [...it]; } catch(e) { if (e instanceof TypeError) n++; } it[Symbol.iterator] = function() { return 1; }; try { [...it]; } catch(e) { if (e instanceof TypeError) n++; } it[Symbol.iterator] = function() { return {next() { return 1; }}; }; try { [...it]; } catch(e) { if (e instanceof TypeError) n++; } n === 3",
        "let closed = 0; let it = {next() { throw 17; }, return() { closed++; return {}; }}; it[Symbol.iterator] = function() { return this; }; let error; try { [...it]; } catch(e) { error = e; } error === 17 && closed === 0",
        "function f(a, b) { return this.n + a + b; } let o = {n: 10}; f.bind(o, 1)(...[2]) === 13 && Reflect.apply(f, o, [...[1, 2]]) === 13",
    ] {
        assert_eq!(eval(source), ValueView::Bool(true), "{source}");
    }
    let (_, _, line, column) = failure_at("let x = [...1];");
    assert_eq!((line, column), (1, 10));
    assert_eq!(
        eval(
            "let n = 0; let it = {next() { return {value: ++n, done: false}; }}; it[Symbol.iterator] = function() { return this; }; let caught = false; try { (function() {})(...it); } catch(e) { caught = e instanceof RangeError; } caught && n === 65536"
        ),
        ValueView::Bool(true)
    );
    let rt = Runtime::with_limits(clj_quickjs::VmLimits {
        max_stack_values: 128,
        ..clj_quickjs::VmLimits::DEFAULT
    });
    let task = rt
        .spawn(&exe("(function() {})(...new Array(200))"), &[])
        .unwrap();
    match task.run().unwrap() {
        RunOutcome::Failed(error) => assert_eq!(error.kind(), VmErrorKind::StackOverflow),
        other => panic!("spread arguments bypassed stack limits: {other:?}"),
    }
}

#[test]
fn generator_next_and_yield() {
    // QuickJS test_generator semantic adaptation, tests/test_builtin.js:
    // Copyright (c) 2017-2021 Fabrice Bellard
    // Copyright (c) 2017-2021 Charlie Gordon
    // Permission is hereby granted, free of charge, to any person obtaining a copy
    // of this software and associated documentation files (the "Software"), to deal
    // in the Software without restriction, including without limitation the rights
    // to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
    // copies of the Software, and to permit persons to whom the Software is
    // furnished to do so, subject to the following conditions:
    // The above copyright notice and this permission notice shall be included in
    // all copies or substantial portions of the Software.
    // THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
    // IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
    // FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL
    // THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
    // LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
    // OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
    // THE SOFTWARE.
    // Synthetic fixture, not light-cljs output. The four f() rows follow
    // QuickJS tests/test_builtin.js test_generator at 7d1c13c5d8f8;
    // rewritten assertions and `let` bindings, rather than copied text.
    assert_eq!(
        eval(
            r#"
        let calls = 0;
        function* f(arg) {
            calls++;
            if (arg !== 9) throw 'argument';
            yield 1;
            let ret = yield 2;
            if (ret !== 'next_arg') throw 'resume argument';
            return 3;
        }
        let g = f(9);
        if (calls !== 0) throw 'eager body';
        let a = g.next(99);
        let b = g.next();
        let c = g.next('next_arg');
        let d = g.next();
        if (a.value !== 1 || a.done !== false || b.value !== 2 || b.done !== false ||
            c.value !== 3 || c.done !== true || d.value !== undefined || d.done !== true)
            throw 'rows';
        function* empty(x) { return x; }
        let e = empty(7).next();
        if (e.value !== 7 || !e.done) throw 'empty';
        let h = f(9), i = f(9);
        if (h.next().value !== 1 || i.next().value !== 1 || h.next().value !== 2)
            throw 'independence';
        let reentrant;
        function* reenter() {
            try { reentrant.next(); } catch(e) { yield e instanceof TypeError; }
        }
        reentrant = reenter();
        if (reentrant.next().value !== true) throw 'reentrant';
        function* broken() { throw 42; }
        let bad = broken(), caught = 0;
        try { bad.next(); } catch(e) { caught = e; }
        let end = bad.next();
        if (caught !== 42 || end.value !== undefined || !end.done) throw 'throw';
        let wrong = 0;
        try { g.next.call({}); } catch(e) { wrong = e instanceof TypeError; }
        if (!wrong) throw 'receiver';
        let expr = function*(x) { yield x; };
        if (expr(8).next().value !== 8) throw 'expression';
        true
    "#
        ),
        ValueView::Bool(true)
    );
}

#[test]
fn generator_rejected_resume_preserves_suspension() {
    // Synthetic fixture, not light-cljs output. The deepest caller fits,
    // but attaching the generator above it exceeds the task's frame limit.
    let rt = Runtime::with_limits(clj_quickjs::VmLimits {
        max_call_depth: 3,
        ..clj_quickjs::VmLimits::default()
    });
    let script = exe(r#"
        function* f() { yield 7; return 8; }
        let g = f();
        function nested() { return g.next(); }
        function deep() { return nested(); }
        let caught = false;
        try { deep(); } catch(e) { caught = true; }
        let a = g.next();
        let b = g.next();
        caught && a.value === 7 && !a.done && b.value === 8 && b.done
    "#);
    let result = rt.spawn(&script, &[]).unwrap().run().unwrap();
    assert!(matches!(result, RunOutcome::Complete(v) if v.view() == ValueView::Bool(true)));
}

#[test]
fn generator_branding_uses_prototype() {
    // Synthetic fixtures, not light-cljs output. General @@toStringTag
    // consumption remains explicitly unsupported, as for other iterators.
    assert_eq!(
        eval(
            "function* g() {} let a = g(); let p = Object.getPrototypeOf(g.prototype); let d = Object.getOwnPropertyDescriptor(p, Symbol.toStringTag); a[Symbol.toStringTag] === 'Generator' && d.value === 'Generator' && !d.writable && !d.enumerable && d.configurable"
        ),
        ValueView::Bool(true)
    );
    assert_eq!(
        eval(
            "function* g() {} g.prototype = {}; Object.prototype.toString.call(g()) === '[object Object]'"
        ),
        ValueView::Bool(true)
    );
    assert_eq!(
        failure("function* g() {} Object.prototype.toString.call(g())").kind(),
        VmErrorKind::UnsupportedOperand
    );
}

#[test]
fn generator_throw_preserves_exception_diagnostics() {
    // Synthetic fixtures, not light-cljs output.
    for (source, throw_line, caller_line) in [
        ("function* g() {\nthrow 42;\n}\ng().next();", 2, 4),
        (
            "function* g() {\ntry {\nthrow 42;\n} finally { let x = 1; }\n}\ng().next();",
            3,
            6,
        ),
    ] {
        let error = failure(source);
        assert_eq!(error.kind(), VmErrorKind::Thrown);
        assert_eq!(error.diagnostic().source.unwrap().line, throw_line);
        assert_eq!(error.diagnostic().call_chain.len(), 1);
        assert_eq!(
            error.diagnostic().call_chain[0].source.unwrap().line,
            caller_line
        );
    }
}

#[test]
fn generator_preserves_operands_and_handlers_across_callers() {
    // Synthetic fixtures, not light-cljs output. Move suspended handlers
    // between shallower and deeper JavaScript callers in both directions.
    for source in [
        "function* f() { return 2 + (yield 1); } let g=f(); function a() { return b(); } function b() { return g.next(5); } let x=a(); let y=g.next(5); x.value===1 && !x.done && y.value===7 && y.done",
        "function* f() { try { let n=yield 1; throw n; } catch(e) { return e+4; } } let g=f(); function a() { return b(); } function b() { return g.next(3); } let x=g.next(); let y=a(); x.value===1 && !x.done && y.value===7 && y.done",
        "function* f() { try { return {n:7}; } finally { yield 1; } } let g=f(); function a() { return b(); } function b() { return g.next(); } let x=a(); let y=g.next(); x.value===1 && !x.done && y.value.n===7 && y.done",
        "function* f() { try { throw {n:7}; } finally { yield 1; } } let g=f(); function a() { return b(); } function b() { return g.next(); } let x=g.next(); let n=0; try { a(); } catch(e) { n=e.n; } x.value===1 && !x.done && n===7 && g.next().done",
    ] {
        assert_eq!(eval(source), ValueView::Bool(true), "{source}");
    }
}

#[test]
fn generator_yield_keeps_operands_and_handlers() {
    // Synthetic fixtures, not light-cljs output. The f3 row adapts
    // QuickJS tests/test_builtin.js::test_generator at 7d1c13c5d8f8,
    // using let instead of var (clj-quickjs-94) and explicit result checks.
    // Copyright (c) 2017-2021 Fabrice Bellard
    // Copyright (c) 2017-2021 Charlie Gordon
    // Permission is hereby granted, free of charge, to any person obtaining a copy
    // of this software and associated documentation files (the "Software"), to deal
    // in the Software without restriction, including without limitation the rights
    // to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
    // copies of the Software, and to permit persons to whom the Software is
    // furnished to do so, subject to the following conditions:
    // The above copyright notice and this permission notice shall be included in
    // all copies or substantial portions of the Software.
    // THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
    // IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
    // FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL
    // THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
    // LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
    // OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
    // THE SOFTWARE.
    let cases = [
        (
            "f3",
            "let ret; try { ret = 2 + (yield 1); } catch(e) {} finally { ret++; cleaned++; } return ret;",
            "let a = first(g); if (cleaned !== 0) throw 'early finally'; let b = second(g, 3); a.value === 1 && a.done === false && b.value === 6 && b.done === true && cleaned === 1",
        ),
        (
            "several operands",
            "let a = 2, b = 3, c = 4; return a + b * (yield c);",
            "let a = first(g); let b = second(g, 10); a.value === 4 && !a.done && b.value === 32 && b.done",
        ),
        (
            "catch body",
            "try { throw {n: 7}; } catch(e) { return e.n + (yield e.n + 1); } finally { cleaned++; }",
            "let a = first(g); if (cleaned !== 0) throw 'early finally'; let b = second(g, 10); a.value === 8 && !a.done && b.value === 17 && b.done && cleaned === 1",
        ),
        (
            "pending return",
            "try { return 1; } finally { cleaned++; yield 2; cleaned++; }",
            "let a = first(g); if (cleaned !== 1) throw 'early finally tail'; let b = second(g); a.value === 2 && !a.done && b.value === 1 && b.done && cleaned === 2",
        ),
        (
            "throw after resume",
            "try { throw (yield 1); } catch(e) { let n = yield e.n; return n + e.n; } finally { cleaned++; }",
            "let a = first(g); let b = second(g, {n: 7}); if (cleaned !== 0) throw 'early finally'; let c = first(g, 10); a.value === 1 && !a.done && b.value === 7 && !b.done && c.value === 17 && c.done && cleaned === 1",
        ),
        (
            "pending throw",
            "try { try { throw {n: 7}; } finally { yield 2; cleaned++; } } catch(e) { return e.n; }",
            "let a = first(g); let b = second(g); a.value === 2 && !a.done && b.value === 7 && b.done && cleaned === 1",
        ),
        (
            "pending jump and normal completion",
            "let n = 0; while (n < 1) { try { n++; break; } finally { yield 2; cleaned++; } } try { n++; } finally { yield 3; cleaned++; } return n;",
            "let a = first(g); let b = second(g); let c = first(g); a.value === 2 && !a.done && b.value === 3 && !b.done && c.value === 2 && c.done && cleaned === 2",
        ),
    ];
    for (name, body, checks) in cases {
        for (first, second) in [("one", "three"), ("three", "one")] {
            // Exactly one or three JS helper frames surround next. The
            // deeper path also retains callees, arguments and extra locals
            // below the activation, so its new stack base must be rebased.
            let source = format!(
                "function keep(padding, value) {{ return value; }}
                 function one(g, value) {{ return g.next(value); }}
                 function three(g, value) {{ let x = 11, y = 12; return keep(x + y, two(g, value)); }}
                 function two(g, value) {{ return keep(13, one(g, value)); }}
                 let first = {first}, second = {second}, cleaned = 0;
                 function* f() {{ {body} }} let g = f();
                 {checks}"
            );
            assert_eq!(eval(&source), ValueView::Bool(true), "{name}: {first}");
        }
    }
}

#[test]
fn generator_call_protocols_and_prototypes() {
    // Synthetic fixtures, not light-cljs output.
    for source in [
        "let n=0; function* g(x=++n) { yield x; } let a=g(); n===1 && a.next().value===1",
        "function* g() { yield arguments[1]+this.n; } g.call({n:3},0,4).next().value===7",
        "function* g(x) { yield x; } g.apply(null,[4]).next().value===4 && g.bind(null,5)().next().value===5",
        "function* g(x) { yield x; } __rt.reduce((a,x)=>a+g(x).next().value,0,__rt.vector(1,2))===3",
        "function* g() {} let a=g(); Object.getPrototypeOf(a)===g.prototype && Object.getPrototypeOf(g.prototype).next===a.next",
        "function* g() {} let p={x:3}; g.prototype=p; Object.getPrototypeOf(g())===p",
        "function* g() {} let next=g().next; g.prototype=0; next.call(g()).done",
        "function* g() {} let a=g(); a.x=2; Object.defineProperty(a,'y',{value:3}); Object.keys(a)[0]==='x' && a.x+a.y===5 && delete a.x && !('x' in a)",
        "function* g() { yield 1; } let a=g(); Object.freeze(a); Object.isFrozen(a) && a.next().value===1",
        "let caught=false; try { new (function*(){})(); } catch(e) { caught=e instanceof TypeError; } caught",
        "let o={*g(x) { yield this.n+x; },n:2}; o.g(5).next().value===7",
        "function* g() { yield 2; yield 3; } let it=g(); it[Symbol.iterator]()===it && Object.getPrototypeOf(Object.getPrototypeOf(g.prototype))===Object.getPrototypeOf(Object.getPrototypeOf([].values()))",
        "function* g() { yield 2; yield 3; } let sum=0; for (let n of g()) { sum+=n; } sum===5",
        "function* g() { yield 2; yield 3; } let a=[...g()]; a.length===2 && a[0]===2 && a[1]===3",
        "function* g() { yield 2; yield 3; } function sum(a,b) { return a+b; } sum(...g())===5",
        "function* g() { yield 2; yield 3; } Array.from(g(), n=>n*2).join(',')==='4,6'",
    ] {
        assert_eq!(eval(source), ValueView::Bool(true), "{source}");
    }
}

/// Synthetic fixtures, not light-cljs output.
#[test]
fn array_from_iterables_and_array_likes() {
    for src in [
        "let a = [1, 2, 3], b = Array.from(a); b !== a && b.length === 3 && b.join() === '1,2,3'",
        "Array.from({length: 2, 0: 'a', 1: 'b'}).join() === 'a,b'",
        "Array.from([1, 2], function(x, i) { return x * 10 + i; }).join() === '10,21'",
        "Array.from([1, 2], function(x, i) { return this.n + x + i; }, {n: 5}).join() === '6,8'",
        "let n = 0; let a = Array.from({length: 2, 0: 1, 1: 2}, function(x, i) { n++; return this.offset + x * 10 + i; }, {offset: 100}); a.join() === '110,121' && n === 2",
        "Array.from.call(null, [1])[0] === 1 && Array.from.call(() => 0, {length: 1, 0: 2})[0] === 2 && Array.from.call(Math.floor, [3])[0] === 3",
        "Array.from.call((() => 0).bind(null), [4])[0] === 4 && Array.from.call(BigInt, [5])[0] === 5",
        "let n = 0; try { Array.from(null); } catch(e) { if (e instanceof TypeError) n++; } try { Array.from(undefined); } catch(e) { if (e instanceof TypeError) n++; } n === 2",
        "let n = 0; let o = {}; o[Symbol.iterator] = function() { return {next() { n++; return {value: n, done: n > 3}; }}; }; let a = Array.from(o, function(x, i) { return n === i + 1 ? x : 99; }); a.join() === '1,2,3' && n === 4",
        "let ok = false; try { Array.from([1], 5); } catch(e) { ok = e instanceof TypeError; } ok",
        "let n = 0, c = 0; let o = {next() { return {value: ++n}; }, return() { c++; return {}; }}; o[Symbol.iterator] = function() { return this; }; let ok = false; try { Array.from(o, function(x) { if (x === 2) throw 17; return x; }); } catch(e) { ok = e === 17; } ok && n === 2 && c === 1",
        "let reads = 0, o = {}; Object.defineProperty(o, Symbol.iterator, {get() { reads++; throw 99; }}); let ok = false; try { Array.from(o, null); } catch(e) { ok = e instanceof TypeError; } ok && reads === 0",
        "Array.from('A\\u{1f600}B').length === 3 && Array.from(new Uint8Array([7, 8])).join() === '7,8'",
        "let a = Array.from({length: 2.9, 1: 'x'}); a.length === 2 && Object.getOwnPropertyDescriptor(a, '0').writable && a[0] === undefined && a[1] === 'x'",
        "Array.from({length: -1}).length === 0 && Array.from({length: NaN}).length === 0 && Array.from(5).length === 0 && Array.from(true).length === 0 && Array.from(Symbol()).length === 0",
        "let ok = false; try { Array.from({length: Infinity}); } catch(e) { ok = e instanceof RangeError; } ok",
        "let n = 0, o = {length: {valueOf() { n++; return 2; }}, 0: 3, 1: 4}; Array.from(o).join() === '3,4' && n === 1",
        "let o = {length: 1, 0: 3}; o[Symbol.iterator] = null; Array.from(o)[0] === 3",
        "let o = {length: 1}; o[Symbol.iterator] = 5; let ok = false; try { Array.from(o); } catch(e) { ok = e instanceof TypeError; } ok",
        "let c = 0, o = {next() { throw 17; }, return() { c++; return {}; }}; o[Symbol.iterator] = function() { return this; }; let ok = false; try { Array.from(o); } catch(e) { ok = e === 17; } ok && c === 0",
        "let c = 0, o = {next() { return {get done() { throw 17; }}; }, return() { c++; return {}; }}; o[Symbol.iterator] = function() { return this; }; let ok = false; try { Array.from(o); } catch(e) { ok = e === 17; } ok && c === 0",
        "let c = 0, o = {next() { return {get value() { throw 17; }}; }, return() { c++; return {}; }}; o[Symbol.iterator] = function() { return this; }; let ok = false; try { Array.from(o); } catch(e) { ok = e === 17; } ok && c === 0",
        "let c = 0, o = {next() { return {value: 1}; }, get return() { c++; throw 99; }}; o[Symbol.iterator] = function() { return this; }; let ok = false; try { Array.from(o, function() { throw 17; }); } catch(e) { ok = e === 17; } ok && c === 1",
        "let c = 0, o = {next() { return {value: 1}; }, return() { c++; throw 99; }}; o[Symbol.iterator] = function() { return this; }; let ok = false; try { Array.from(o, function() { throw 17; }); } catch(e) { ok = e === 17; } ok && c === 1",
        "let o = {next() { return {value: 1}; }, return() { return 5; }}; o[Symbol.iterator] = function() { return this; }; let ok = false; try { Array.from(o, function() { throw 17; }); } catch(e) { ok = e === 17; } ok",
        "let d = Object.getOwnPropertyDescriptor(Array, 'from'); Array.from.name === 'from' && Array.from.length === 1 && d.writable && d.configurable && !d.enumerable",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
    assert_eq!(
        failure("Array.from.call(function C() {}, [1])").kind(),
        VmErrorKind::UnsupportedOperand,
    );
}

/// Synthetic fixtures, not light-cljs output.
#[test]
fn array_from_iterator_protocol_errors_and_exhaustion() {
    for src in [
        "let c = 0, o = {next() { return 1; }, return() { c++; return {}; }}; o[Symbol.iterator] = function() { return this; }; let ok = false; try { Array.from(o); } catch(e) { ok = e instanceof TypeError; } ok && c === 0",
        "let c = 0, o = {next: 1, return() { c++; return {}; }}; o[Symbol.iterator] = function() { return this; }; let ok = false; try { Array.from(o); } catch(e) { ok = e instanceof TypeError; } ok && c === 0",
        "let n = 0, reads = 0, o = {next() { n++; return {done: true, get value() { reads++; throw 99; }}; }, get return() { reads++; throw 98; }}; o[Symbol.iterator] = function() { return this; }; let a = Array.from(o, function() { throw 97; }); a.length === 0 && n === 1 && reads === 0",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
}

#[test]
fn array_from_close_preserves_exception_diagnostics() {
    // Synthetic fixture, not light-cljs output.
    let src = "let it = {next() { return {value: 1}; }, return() { throw 99; }};\nit[Symbol.iterator] = function() { return this; };\nArray.from(it, function mapper() {\nthrow 17;\n});";
    let error = failure(src);
    assert_eq!(error.kind(), VmErrorKind::Thrown);
    assert_eq!(error.diagnostic().source.unwrap().line, 4);
    assert!(error.message().contains("17"));
}

/// Synthetic fixtures, not light-cljs output.
#[test]
fn promise_thenable_adoption() {
    #[derive(Default)]
    struct Log(Vec<Primitive>);
    impl Host for Log {
        fn effect(&mut self, call: HostCall) -> Result<Primitive, String> {
            self.0.push(call.arg);
            Ok(Primitive::Undefined)
        }
    }
    for (source, expected) in [
        (
            r#"
            new Promise(r => r(Promise.resolve(1))).then(v => __rt.effect(0, v));
            Promise.resolve().then(() => __rt.effect(0, 10))
                .then(() => __rt.effect(0, 20)).then(() => __rt.effect(0, 30));
            __rt.effect(0, 0);
        "#,
            vec![0., 10., 20., 1., 30.],
        ),
        (
            r#"
            let calls = 0; let reads = 0;
            const value = {get then() { reads++; return function(res, rej) {
                __rt.effect(0, ++calls); __rt.effect(0, this === value ? 2 : 99);
                res(5); res(6); rej(7); throw 8;
            }; }};
            Promise.resolve(value).then(v => { __rt.effect(0, v); __rt.effect(0, reads); });
            __rt.effect(0, calls);
        "#,
            vec![0., 1., 2., 5., 1.],
        ),
        (
            r#"
            new Promise(r => r({then(res) { res(5); }})).then(v => __rt.effect(0, v));
            Promise.resolve({then() { throw 7; }}).catch(v => __rt.effect(0, v));
            Promise.resolve({get then() { throw 9; }}).catch(v => __rt.effect(0, v));
            new Promise(r => r(Promise.reject(11))).catch(v => __rt.effect(0, v));
        "#,
            vec![9., 5., 7., 11.],
        ),
        (
            r#"
            let late;
            new Promise((r, j) => {
                late = r;
                r({then(res, rej) { late(99); res({then(r) { r(12); }}); rej(98); throw 97; }});
                j(96); throw 95;
            }).then(v => __rt.effect(0, v));
            Promise.resolve(1).then(() => Promise.resolve(13)).then(v => __rt.effect(0, v));
        "#,
            vec![12., 13.],
        ),
    ] {
        let rt = Runtime::new();
        let task = rt
            .spawn(&compile_script("thenable.js", source).unwrap(), &[])
            .unwrap();
        let mut log = Log::default();
        let outcome = task.run_with(Budget::UNLIMITED, &mut log).unwrap();
        assert!(matches!(outcome, RunOutcome::Complete(_)), "{outcome:?}");
        assert_eq!(
            log.0,
            expected
                .into_iter()
                .map(Primitive::Number)
                .collect::<Vec<_>>()
        );
    }
}

/// Synthetic fixtures, not light-cljs output.
#[test]
fn for_of_loops() {
    assert_eq!(
        eval("let sum = 0; for (const x of [1, 2, 3]) sum += x; sum"),
        ValueView::Number(6.0)
    );
    assert_eq!(
        eval(
            "let fs = []; for (let x of [1, 2, 3]) fs.push(() => x); fs[0]() === 1 && fs[2]() === 3"
        ),
        ValueView::Bool(true)
    );
    assert_eq!(
        eval(
            r#"
        let calls = 0, closes = 0;
        let iterable = {}; iterable[Symbol.iterator] = function() { return {
            next() { calls++; return { value: calls, done: calls > 3 }; },
            return() { closes++; return {}; }
        }; };
        let sum = 0;
        for (const x of iterable) { if (x === 2) continue; sum += x; }
        let exhausted = calls === 4 && closes === 0 && sum === 4;
        calls = 0;
        for (const x of iterable) break;
        function f() { for (const x of iterable) return 42; }
        calls = 0;
        let returned = f() === 42;
        let thrown = false;
        calls = 0;
        try { for (const x of iterable) throw 17; } catch (e) { thrown = e === 17; }
        exhausted && returned && thrown && closes === 3
    "#
        ),
        ValueView::Bool(true)
    );
    assert_eq!(
        eval(
            r#"
        let closed = 0;
        let it = { next() { return {value: 1, done: false}; },
                   return() { closed++; throw 99; } };
        it[Symbol.iterator] = function() { return this; };
        let original = false, replaced = false;
        try { for (let x of it) throw 17; } catch (e) { original = e === 17; }
        try { for (let x of it) break; } catch (e) { replaced = e === 99; }
        original && replaced && closed === 2
    "#
        ),
        ValueView::Bool(true)
    );
    assert_eq!(
        eval(
            r#"
        let it = { next() { return {value: 1, done: false}; }, return() { return 1; } };
        it[Symbol.iterator] = function() { return this; };
        let a = false, b = false;
        try { for (const x of it) break; } catch (e) { a = e instanceof TypeError; }
        try { for (const x of 1) {} } catch (e) { b = e instanceof TypeError; }
        a && b
    "#
        ),
        ValueView::Bool(true)
    );
}

/// Synthetic fixtures, not light-cljs output. Live array lengths, code
/// points, generic receivers and detached typed arrays use actual iterators.
#[test]
fn for_of_builtin_iterators() {
    for src in [
        "let s = ''; for (let x of 'A\\u{1f600}\\ud800B') s += x.length; s === '1211'",
        "let a = [1, , 3], s = ''; for (let x of a) { s += String(x); if (x === 1) a.push(4); } s === '1undefined34'",
        "let a = [1], it = a.values(); it[Symbol.iterator]() === it && it.next().value === 1 && it.next().done && (a.push(2), it.next().done)",
        "let it = Array.prototype.values.call({0: 5, length: 1}); it.next().value === 5 && it.next().done",
        "let it = Array.prototype.values.call('ab'); it.next().value === 'a' && it.next().value === 'b' && it.next().done",
        "Array.prototype.values === Array.prototype[Symbol.iterator] && Uint8Array.prototype.values === Uint8Array.prototype[Symbol.iterator]",
        "let s = 0; for (let x of new Uint16Array([1, 2, 3])) s += x; s === 6",
        "let a = new Uint8Array([2]); let it = a.values(); a.buffer.transfer(); let caught = false; try { it.next(); } catch (e) { caught = e instanceof TypeError; } caught",
        "let it = 'ab'[Symbol.iterator](); it[Symbol.iterator]() === it && it.next().value === 'a' && it.next().value === 'b' && it.next().done",
        "[1].values()[Symbol.toStringTag] === 'Array Iterator' && 'a'[Symbol.iterator]()[Symbol.toStringTag] === 'String Iterator'",
        "let s = ''; for (var x of [1, 2]) s += x; s === '12' && x === 2",
        "let x = 0; for (x of [1, 2]) {} x === 2",
        "let caught = false; try { for (let x of x) {} } catch (e) { caught = e instanceof ReferenceError; } caught",
        "let n = 0; for (let x of [1, 2]) { let f = () => x; x += 10; n += f(); } n === 23",
        "let n = 0; for (const x of [1, 2]) { try { x = 3; } catch (e) { if (e instanceof TypeError) n++; } } n === 2",
        "let c = 0; try { for (let x of true) {} } catch (e) { if (e instanceof TypeError) c++; } try { for (let x of Symbol()) {} } catch (e) { if (e instanceof TypeError) c++; } c === 2",
        "let caught = false; try { 'a'[Symbol.iterator]().next.call([1].values()); } catch (e) { caught = e instanceof TypeError; } caught",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
}

#[test]
fn for_of_borrowed_array_iterator_on_typed_array() {
    for src in [
        "let a = new Uint8Array([7]); Object.defineProperty(a, 'length', {value: 0}); let it = Array.prototype.values.call(a); it.next().value === 7 && it.next().done",
        "let a = new Uint8Array([7]); Object.defineProperty(a, 'length', {get() { throw 99; }}); Array.prototype.values.call(a).next().value === 7",
        "let a = new Uint8Array([7]); let it = Array.prototype.values.call(a); a.buffer.transfer(); let caught = false; try { it.next(); } catch (e) { caught = e instanceof TypeError; } caught",
        "let a = new Uint8Array([7]); a.buffer.transfer(); let it = Array.prototype.values.call(a); let caught = false; try { it.next(); } catch (e) { caught = e instanceof TypeError; } caught",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
}

/// Synthetic fixture, not light-cljs output. The order of cleanup and
/// precedence of exceptions must match IteratorClose, even through finally.
#[test]
fn for_of_abrupt_completions() {
    assert_eq!(
        eval(
            r#"
        let log = '', n = 0;
        let it = { next() { return { value: ++n, done: false }; }, return() { log += 'C'; return {}; } };
        it[Symbol.iterator] = function() { return this; };
        outer: for (let x of [1, 2]) {
            for (let y of it) { try { if (x === 1) continue outer; break outer; } finally { log += 'F'; } }
        }
        log === 'FCFC'
    "#
        ),
        ValueView::Bool(true)
    );
    assert_eq!(
        eval(
            r#"
        let n = 0, c = 0;
        let it = { next() { n++; return {value: n, done: n > 2}; }, return() { c++; return {}; } };
        it[Symbol.iterator] = function() { return this; };
        same: for (let x of it) { try { continue same; } finally {} }
        n === 3 && c === 0
    "#
        ),
        ValueView::Bool(true)
    );
    assert_eq!(
        eval(
            r#"
        let closes = 0, caught = 0;
        let it = { next() { throw 7; }, return() { closes++; return {}; } };
        it[Symbol.iterator] = function() { return this; };
        try { for (const x of it) {} } catch (e) { if (e === 7) caught++; }
        it.next = function() { return { get done() { throw 7; } }; };
        try { for (const x of it) {} } catch (e) { if (e === 7) caught++; }
        it.next = function() { return { done: false, get value() { throw 7; } }; };
        try { for (const x of it) {} } catch (e) { if (e === 7) caught++; }
        it.next = function() { return 1; };
        try { for (const x of it) {} } catch (e) { if (e instanceof TypeError) caught++; }
        caught === 4 && closes === 0
    "#
        ),
        ValueView::Bool(true)
    );
    assert_eq!(
        eval(
            r#"
        let count = 0, caught = 0;
        let it = { next() { return {value: 1, done: false}; }, get return() { count++; throw 99; } };
        it[Symbol.iterator] = function() { return this; };
        try { for (let x of it) throw 7; } catch (e) { if (e === 7) caught++; }
        try { for (let x of it) break; } catch (e) { if (e === 99) caught++; }
        function f() { for (let x of it) return 1; }
        try { f(); } catch (e) { if (e === 99) caught++; }
        caught === 3 && count === 3
    "#
        ),
        ValueView::Bool(true)
    );
    assert_eq!(
        eval(
            r#"
        let gets = 0, n = 0, closed = 0;
        let it = { get next() { gets++; return function() { n++; return { value: n, done: n > 2 }; }; }, return() { closed++; return {}; } };
        it[Symbol.iterator] = function() { return this; };
        let total = 0; for (let x of it) total += x;
        total === 3 && gets === 1 && closed === 0
    "#
        ),
        ValueView::Bool(true)
    );
    assert_eq!(
        eval(
            r#"
        let closed = 0, caught = false;
        let it = { next() { return {value: 1, done: false}; }, return() { closed++; return {}; } };
        it[Symbol.iterator] = function() { return this; };
        const k = 0;
        try { for (k of it) {} } catch (e) { caught = e instanceof TypeError; }
        caught && closed === 1
    "#
        ),
        ValueView::Bool(true)
    );
}

#[test]
fn for_of_close_preserves_exception_diagnostics() {
    let src = "let it = {\nnext() { return {value: 1, done: false}; },\nreturn() { throw 99; }\n};\nit[Symbol.iterator] = function() { return this; };\nfor (let x of it) break;";
    let error = failure(src);
    assert_eq!(error.kind(), VmErrorKind::Thrown);
    assert_eq!(error.diagnostic().source.unwrap().line, 3);
    let error = failure(&src.replace("break;", "throw 17;"));
    assert_eq!(error.diagnostic().source.unwrap().line, 6);
    assert!(error.message().contains("17"));
}

#[test]
fn upstream_string_rope() {
    assert_eq!(
        eval(include_str!("fixtures/js/quickjs_rope.js")),
        ValueView::Bool(true)
    );
}

/// Synthetic fixture, not light-cljs output. JavaScript Map uses the same
/// content hash and equality for flat strings and append/prepend ropes.
#[test]
fn rope_strings_are_javascript_map_keys() {
    assert_eq!(
        eval(
            r#"
        let appended = ''; let prepended = ''; let units = [];
        for (let i = 0; i < 100; i++) {
            appended += String.fromCharCode(i);
            prepended = String.fromCharCode(99 - i) + prepended;
            units.push(i);
        }
        let flat = String.fromCharCode.apply(null, units);
        let m = new Map();
        m.set(appended, 1);
        let found = m.get(prepended) === 1 && m.has(flat);
        m.set(flat, 2);
        found && m.size === 1 && m.get(prepended) === 2 &&
        m.delete(prepended) && !m.has(appended) && m.size === 0
    "#
        ),
        ValueView::Bool(true)
    );
}

/// Synthetic fixture, not light-cljs output. Persistent map key comparison,
/// property interning, publication and indexed search all read rope content.
#[test]
fn rope_strings_are_content_keys_and_publishable() {
    assert_eq!(
        eval(
            r#"
        let s = '';
        for (let i = 0; i < 100; i++) s += 'x';
        let flat = ''.padEnd(100, 'x');
        let m = __rt.map(s, 42);
        let o = {}; o[s] = 17;
        __rt.get(m, flat) === 42 && o[flat] === 17 && s === flat &&
        !(s < flat) && !(s > flat) && s.indexOf('xxx', 90) === 90
    "#
        ),
        ValueView::Bool(true)
    );
    let rt = Runtime::new();
    let task = rt
        .spawn(
            &exe("let s = ''; for (let i = 0; i < 10000; i++) s += 'x'; s"),
            &[],
        )
        .unwrap();
    let RunOutcome::Complete(value) = task.run().unwrap() else {
        panic!("expected rope")
    };
    rt.publish(&value).unwrap();
    let atom = rt.new_atom(&value).unwrap();
    rt.collect().unwrap();
    let reader = compile_function(
        "function read(s) { return s.length === 10000 && s.charCodeAt(9999) === 120; }",
    )
    .unwrap();
    let task = rt.spawn(&reader, &[&rt.atom_load(&atom).unwrap()]).unwrap();
    assert!(
        matches!(task.run().unwrap(), RunOutcome::Complete(v) if v.view() == ValueView::Bool(true))
    );
}

/// Synthetic fixtures, not light-cljs output.
#[test]
fn bigint_literals_and_strict_equality() {
    for src in [
        "515377520732011331036461129765621272702107522001n === 515377520732011331036461129765621272702107522001n",
        "0x5a4653ca673768565b41f775d6947d55cf3813d1n === 515377520732011331036461129765621272702107522001n",
        "0xffffffffffffffffn === 18446744073709551615n",
        "0o77777777777n === 8589934591n",
        "0o17n === 15n && 0b101n === 5n && 0n === 0n",
        "1n !== 1 && !(1n === 1) && typeof 1n === 'bigint'",
        "(0n ? 1 : 2) === 2 && (1n ? 1 : 2) === 1 && !0n && !!1n",
        "let x = 0; if (0n) x = 1; if (1n) x = 2; x === 2",
        "(0n || 2) === 2 && (1n && 3) === 3 && (0n && 3) === 0n && (1n || 3) === 1n",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
    let octal = format!("0o{}n === 79228162514264337593543950335n", "7".repeat(32));
    assert_eq!(eval(&octal), ValueView::Bool(true));
    let rt = Runtime::new();
    let task = rt
        .spawn(
            &exe("515377520732011331036461129765621272702107522001n"),
            &[],
        )
        .unwrap();
    let RunOutcome::Complete(value) = task.run().unwrap() else {
        panic!("BigInt completion")
    };
    assert_eq!(value.view(), ValueView::BigInt);
    assert_eq!(
        value.bigint().unwrap().to_string(),
        "515377520732011331036461129765621272702107522001"
    );
    let host = rt.root(Primitive::BigInt(clj_quickjs::JsBigInt::from(i64::MIN)));
    let inspect =
        compile_function("function f(n) { return !!n && typeof n === 'bigint'; }").unwrap();
    assert!(
        matches!(rt.spawn(&inspect, &[&host]).unwrap().run().unwrap(), RunOutcome::Complete(v) if v.view() == ValueView::Bool(true))
    );
    let exact = rt.scope(|scope| {
        let local = scope.primitive(Primitive::BigInt(value.bigint().unwrap()));
        assert_eq!(scope.view(local), ValueView::BigInt);
        scope.bigint(local).unwrap()
    });
    drop(value);
    drop(task);
    rt.collect().unwrap();
    assert_eq!(
        exact.to_string(),
        "515377520732011331036461129765621272702107522001"
    );
}

/// Synthetic fixtures, not light-cljs output.
#[test]
fn bigint_values_publish_and_survive_collection() {
    let rt = Runtime::new();
    let task = rt
        .spawn(
            &exe("__rt.vector(515377520732011331036461129765621272702107522001n)"),
            &[],
        )
        .unwrap();
    let RunOutcome::Complete(vector) = task.run().unwrap() else {
        panic!("vector completion")
    };
    rt.publish(&vector).unwrap();
    drop(task);
    rt.collect().unwrap();
    let reader = compile_function("function f(v) { return __rt.get(v, 0) === 515377520732011331036461129765621272702107522001n; }").unwrap();
    let reader = rt.spawn(&reader, &[&vector]).unwrap();
    assert!(
        matches!(reader.run().unwrap(), RunOutcome::Complete(v) if v.view() == ValueView::Bool(true))
    );
    for src in [
        "__rt.get(__rt.map(10n, 1), 10n) === 1",
        "__rt.get(__rt.map(0xffffffffffffffffn, 7), 18446744073709551615n) === 7",
        "__rt.get(__rt.map(1n, 7, 1, 8), 1n) === 7",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
}

/// Synthetic fixture, not light-cljs output. Comparison rows adapt QuickJS
/// tests/test_bigint.js::test_less/test_eq/test_bigint3 at revision
/// 7d1c13c5d8f8c6b32893786ed35cdcebd88194d2.
/// Copyright (c) 2017-2021 Fabrice Bellard
/// Copyright (c) 2017-2021 Charlie Gordon
///
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
/// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
/// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
/// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
/// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
/// THE SOFTWARE.
#[test]
fn bigint_relational_and_loose_equality() {
    for src in [
        "2n < 3n",
        "!(3n < 2n)",
        "2n <= 3n",
        "!(3n <= 2n)",
        "3n > 2n",
        "!(2n > 3n)",
        "3n >= 2n",
        "!(2n >= 3n)",
        "2n != 3n",
        "!(2n == 3n)",
        "3n == 3n",
        "3n == 3n",
        "!(3n != 3n)",
        "!(3n != 3n)",
        "3n <= 3n",
        "3n <= 3n",
        "!(3n < 3n)",
        "3n >= 3n",
        "3n >= 3n",
        "!(3n > 3n)",
        "2 < 3n",
        "!(3n < 2)",
        "2 <= 3n",
        "!(3n <= 2)",
        "3n > 2",
        "!(2 > 3n)",
        "3n >= 2",
        "!(2 >= 3n)",
        "2 != 3n",
        "!(2 == 3n)",
        "3 == 3n",
        "3n == 3",
        "!(3 != 3n)",
        "!(3n != 3)",
        "3 <= 3n",
        "3n <= 3",
        "!(3 < 3n)",
        "3 >= 3n",
        "3n >= 3",
        "!(3 > 3n)",
        "2.1 < 3n",
        "!(3n < 2.1)",
        "2.1 <= 3n",
        "!(3n <= 2.1)",
        "3n > 2.1",
        "!(2.1 > 3n)",
        "3n >= 2.1",
        "!(2.1 >= 3n)",
        "2.1 != 3n",
        "!(2.1 == 3n)",
        "2 == 2n",
        "2n == 2",
        "!(2 != 2n)",
        "!(2n != 2)",
        "2 <= 2n",
        "2n <= 2",
        "!(2 < 2n)",
        "2 >= 2n",
        "2n >= 2",
        "!(2 > 2n)",
        "100000000000000000000n == 1e20",
        "!(100000000000000000001n == 1e20)",
        "!(9007199254740993n == 9007199254740992)",
        "9007199254740993n > 9007199254740992",
        "0n == -0",
        "!(1n < NaN)",
        "!(1n > NaN)",
        "!(1n == NaN)",
        "1000000000000000000000000n < Infinity",
        "1000000000000000000000000n > -Infinity",
        "1000000000000000000000000n > 1e24",
        "999999999999999983222784n == 1e24",
        "1n == true",
        "!(0n == null)",
        "1n != undefined",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
}

/// Synthetic fixtures, not light-cljs output. Signed BigInts arrive through
/// the host API while unary BigInt minus remains a separate pellet.
#[test]
fn bigint_signed_coercion_and_unordered_comparisons() {
    for src in [
        "2.1 > 2n && 2n < 2.1 && 2.1 != 2n",
        "1n == true && true == 1n && false == 0n && 0n == false",
        "0n != true && true != 0n && 1n != false && false != 1n",
        "0n > -0.5 && -0.5 < 0n && 0n < 0.5 && 0.5 > 0n",
        "0n < 5e-324 && 0n > -5e-324 && 0n != 5e-324",
        "1n < true === false && 0n < true && false < 1n",
        "0n >= null && null <= 0n && !(0n == null) && !(null == 0n)",
        "1n != undefined && undefined != 1n && 1n != Symbol() && Symbol() != 1n",
        "0x10000000000000000n > 0xffffffffffffffffn && 0x8000000000000000n > 0x7fffffffffffffffn",
        "0xffffffffffffffffn == 18446744073709551615n",
        "({valueOf() { return 3n; }}) == 3n && 3n == ({valueOf() { return 3; }})",
        "({valueOf() { return 2n; }}) < 3 && 2 < ({valueOf() { return 3n; }})",
        "let o = {valueOf() { throw 42; }}; let caught = false; try { 1n == o; } catch (e) { caught = e === 42; } caught",
        "let o = {valueOf() { throw 42; }}; let caught = false; try { o < 1n; } catch (e) { caught = e === 42; } caught",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
    for other in ["NaN", "undefined"] {
        for op in ["<", ">", "<=", ">=", "=="] {
            for (a, b) in [("1n", other), (other, "1n")] {
                let src = format!("{a} {op} {b}");
                assert_eq!(eval(&src), ValueView::Bool(false), "{src}");
            }
        }
    }
    for op in ["<", ">", "<=", ">="] {
        for (a, b) in [("1n", "Symbol()"), ("Symbol()", "1n")] {
            let src = format!("{a} {op} {b}");
            assert_eq!(failure(&src).kind(), VmErrorKind::TypeError, "{src}");
        }
    }
    let rt = Runtime::new();
    for (limbs, checks) in [
        (
            vec![2],
            "n > -2.1 && n < -1.9 && n == -2 && -2 == n && n < 0n && n < 0",
        ),
        (
            vec![1, 0, 1],
            "n < -18446744073709551616 && n != -18446744073709551616 && n < 0n && n > -Infinity",
        ),
        (
            vec![0, 0x80000000],
            "n == -9223372036854775808 && n < 0n && n <= -9223372036854775808",
        ),
    ] {
        let n = clj_quickjs::JsBigInt::from_sign_magnitude(true, limbs).unwrap();
        let root = rt.root(Primitive::BigInt(n));
        let script = compile_function(&format!("function f(n) {{ return {checks}; }}")).unwrap();
        let result = rt.spawn(&script, &[&root]).unwrap().run().unwrap();
        assert!(
            matches!(result, RunOutcome::Complete(v) if v.view() == ValueView::Bool(true)),
            "{checks}"
        );
    }
}

/// Synthetic fixture, not light-cljs output. Both objects convert in source
/// order before relational operand swapping; suspended values retain limbs.
#[test]
fn bigint_comparisons_wait_yield_collect_and_migrate() {
    for expression in [
        "a < b",
        "b > a",
        "a <= b",
        "b >= a",
        "a == 9007199254740993n",
        "9007199254740993n == a",
    ] {
        let rt = Runtime::new();
        let source = format!(
            r"
            let log = '';
            let a = {{valueOf() {{ log += 'a'; __rt.need(1971, 1); return 9007199254740993n; }}}};
            let b = {{valueOf() {{ log += 'b'; __rt.need(1972, 1); return 9007199254740994; }}}};
            let result = {expression}; result && log === '{}'
        ",
            if expression.contains('b') {
                if expression.starts_with('b') {
                    "ba"
                } else {
                    "ab"
                }
            } else {
                "a"
            }
        );
        let mut task = rt.spawn(&exe(&source), &[]).unwrap();
        let mut waited = 0;
        let expected_waits = if expression.contains('b') { 2 } else { 1 };
        let mut completed = false;
        for _ in 0..1000 {
            let (next, outcome) = std::thread::spawn(move || {
                let outcome = task
                    .run_with(Budget::instructions(1), &mut NoEffects)
                    .unwrap();
                (task, outcome)
            })
            .join()
            .unwrap();
            task = next;
            match outcome {
                RunOutcome::Yielded => {
                    rt.collect().unwrap();
                }
                RunOutcome::Waiting(_) => {
                    waited += 1;
                    assert!(waited <= expected_waits, "{expression}");
                    rt.collect().unwrap();
                    let request = task.take_producer_requests().pop().unwrap();
                    let value = rt.root(Primitive::Null);
                    task = rt
                        .complete(request.key(), request.producer(), Completion::Ready(&value))
                        .unwrap()
                        .into_runnable()
                        .pop()
                        .unwrap();
                }
                RunOutcome::Complete(value) => {
                    assert_eq!(value.view(), ValueView::Bool(true), "{expression}");
                    assert_eq!(waited, expected_waits);
                    completed = true;
                    break;
                }
                other => panic!("{expression}: {other:?}"),
            }
        }
        assert!(completed, "{expression}");
    }
}

/// Synthetic fixtures, not light-cljs output. BigInt wrapper objects remain
/// deferred; Number conversion is covered separately below.
#[test]
fn bigint_deferred_operations_fail_explicitly() {
    for src in [
        "Object(1n)",
        "Object.prototype.valueOf.call(1n)",
        "Object.prototype.toString.call(1n)",
    ] {
        let error = failure(src);
        assert_eq!(
            error.kind(),
            VmErrorKind::UnsupportedOperand,
            "{src}: {error}"
        );
        assert!(
            error.message().to_ascii_lowercase().contains("bigint"),
            "{src}: {error}"
        );
    }
    for src in [
        "1n + 1",
        "1 + 1n",
        "1n - 1",
        "1 * 1n",
        "1n / 1",
        "1 % 1n",
        "1n ** 1",
        "1 & 1n",
        "1n << 1",
        "1n >>> 1n",
        "1n - {valueOf() { return 1; }}",
        "({valueOf() { return 1n; }}) * 1",
    ] {
        assert_eq!(failure(src).kind(), VmErrorKind::TypeError, "{src}");
    }
    assert_eq!(failure("JSON.stringify(1n)").kind(), VmErrorKind::TypeError);
    assert_eq!(
        eval(
            "let caught = false; try { 1n + 1; } catch (e) { caught = e instanceof TypeError; } caught"
        ),
        ValueView::Bool(true)
    );
}

/// Synthetic fixtures, not light-cljs output. The first three rows mirror
/// QuickJS test_bigint3 (7d1c13c5d8f8); see docs/attribution.md.
/// Copyright (c) 2017-2021 Fabrice Bellard and Charlie Gordon. Permission is hereby granted, free
/// of charge, to any person obtaining a copy of this software and associated
/// documentation files (the "Software"), to deal in the Software without
/// restriction, including without limitation the rights to use, copy, modify,
/// merge, publish, distribute, sublicense, and/or sell copies of the Software,
/// and to permit persons to whom the Software is furnished to do so, subject
/// to the following conditions: The above copyright notice and this permission
/// notice shall be included in all copies or substantial portions of the
/// Software. THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
/// EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
/// MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN
/// NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM,
/// DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR
/// OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE
/// USE OR OTHER DEALINGS IN THE SOFTWARE.
#[test]
fn bigint_to_string_with_radix() {
    for src in [
        "(1n << 100n).toString(10) === '1267650600228229401496703205376'",
        "(-1n << 100n).toString(36) === '-3ewfdnca0n6ld1ggvfgg'",
        "(1n << 100n).toString(8) === '2000000000000000000000000000000000'",
        "(0n).toString() === '0' && (-0n).toString(2) === '0'",
        "(-255n).toString(16) === '-ff' && (255n).toString(2) === '11111111'",
        "(10n ** 2000n).toString().length === 2001",
        "'x' + 12n === 'x12' && 12n + 'x' === '12x' && String(-5n) === '-5'",
        "`${10n}` === '10' && (1n).valueOf() === 1n",
        "(1n).toString() === '1' && String(1n) === '1'",
        "(42n).toString(undefined) === '42' && (255n).toString('16.9') === 'ff'",
        "BigInt.prototype.toString.length === 0 && BigInt.prototype.valueOf.length === 0",
        "BigInt.prototype.toString.call(-255n, 16) === '-ff' && BigInt.prototype.valueOf.call(1n) === 1n",
        "let calls = 0; (255n).toString({valueOf() { calls++; return 16; }}) === 'ff' && calls === 1",
        "let x = {valueOf() { return 12n; }}; 'x' + x === 'x12' && x + 'x' === '12x'",
        "let x = {toString() { return -5n; }}; String(x) === '-5' && `${x}` === '-5'",
        "let o = {}; o[12n] = 7; o['12'] === 7 && ({12: 3})[12n] === 3",
        "[1n, -2n].join(3n) === '13-2' && ''.concat(12n) === '12'",
        "parseInt(255n, 16) === 597 && Symbol.keyFor(Symbol.for(12n)) === '12'",
        "String.raw({raw: [1n, 2n]}, 3n) === '132'",
        "let o = {}; Object.defineProperty(o, 12n, {value: 7, configurable: true}); Object.getOwnPropertyDescriptor(o, 12n).value === 7 && 12n in o && delete o[12n]",
        "new String(12n).valueOf() === '12' && String(Symbol(12n)) === 'Symbol(12)'",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
    for (src, kind) in [
        ("(1n).toString(1)", "RangeError"),
        ("(1n).toString(37)", "RangeError"),
        ("(1n).toString(null)", "RangeError"),
        ("(1n).toString(NaN)", "RangeError"),
        ("(1n).toString(Infinity)", "RangeError"),
        (
            "(1n).toString({valueOf() { return undefined; }})",
            "RangeError",
        ),
        ("(1n).toString(10n)", "TypeError"),
        ("(1n).toString(Symbol())", "TypeError"),
        ("BigInt.prototype.toString.call(1)", "TypeError"),
        ("BigInt.prototype.valueOf.call('1')", "TypeError"),
        ("BigInt.prototype.toString()", "TypeError"),
        ("BigInt.prototype.valueOf()", "TypeError"),
        ("String(BigInt.prototype)", "TypeError"),
        (
            "BigInt.prototype.toString.call(1, {valueOf() { throw 99; }})",
            "TypeError",
        ),
    ] {
        assert_eq!(
            eval(&format!(
                "let ok = false; try {{ {src}; }} catch(e) {{ ok = e instanceof {kind}; }} ok"
            )),
            ValueView::Bool(true),
            "{src}"
        );
    }
}

/// Synthetic fixtures, not light-cljs output. Formatter buffers survive
/// dependency callbacks, collection at yields and execution on new threads.
#[test]
fn bigint_to_string_yields_collects_and_migrates() {
    let digits: String = (0..4000)
        .map(|i| char::from(b'0' + ((i * 7 + 1) % 10) as u8))
        .collect();
    for expression in [
        "value.toString() === expected",
        "String(value).length === 4000",
        "('' + value).length === 4000",
        "`${value}`.length === 4000",
        "[value].join('').length === 4000",
        "String.raw({raw: ['', '']}, value).length === 4000",
        "let o = {}; o[value] = 1; Object.keys(o)[0].length === 4000",
        "let o = {}; Object.defineProperty(o, __rt.need(2021, 1), {value: 1}); Object.keys(o).length === 0 && Object.getOwnPropertyNames(o)[0].length === 4000",
        "Symbol.keyFor(Symbol.for(value)).length === 4000",
        "new Error(value).message.length === 4000",
        "''.concat(value).length === 4000",
        "String.prototype.charAt.call(value, 0) === '1'",
    ] {
        let rt = Runtime::new();
        let source = format!(
            "let expected = '{digits}'; let calls = 0; let value = {{valueOf() {{ calls++; return __rt.need(2021, 1); }}, toString() {{ calls++; return __rt.need(2021, 1); }} }}; {expression}"
        );
        // Primitive toString needs the BigInt receiver; other expressions
        // exercise ToPrimitive callbacks on the object above.
        let source = if expression.starts_with("value.toString()") {
            format!("let value = __rt.need(2021, 1); value.toString() === '{digits}'")
        } else {
            source
        };
        let mut task = rt.spawn(&exe(&source), &[]).unwrap();
        let mut yields = 0;
        let mut waits = 0;
        let mut complete = false;
        for _ in 0..1500 {
            let (next, outcome) = std::thread::spawn(move || {
                let outcome = task
                    .run_with(Budget::instructions(1), &mut NoEffects)
                    .unwrap();
                (task, outcome)
            })
            .join()
            .unwrap();
            task = next;
            match outcome {
                RunOutcome::Yielded => {
                    yields += 1;
                    rt.collect().unwrap();
                }
                RunOutcome::Waiting(_) => {
                    waits += 1;
                    assert_eq!(waits, 1);
                    let request = task.take_producer_requests().pop().unwrap();
                    let value = clj_quickjs::JsBigInt::from_digits(&digits, 10).unwrap();
                    let value = rt.root(Primitive::BigInt(value));
                    task = rt
                        .complete(request.key(), request.producer(), Completion::Ready(&value))
                        .unwrap()
                        .into_runnable()
                        .pop()
                        .unwrap();
                }
                RunOutcome::Complete(value) => {
                    assert_eq!(value.view(), ValueView::Bool(true), "{expression}");
                    assert_eq!(waits, 1);
                    assert!(yields > 100, "formatter must yield: {yields}");
                    complete = true;
                    break;
                }
                other => panic!("{expression}: {other:?}"),
            }
        }
        assert!(complete, "{expression}");
    }
}

/// Synthetic fixtures, not light-cljs output. Locale options apply ToString
/// before validation and must stop before later option getters on failure.
#[test]
fn bigint_to_string_intl_options() {
    for src in [
        "new Intl.Locale('en', {region: 419n}).region === '419'",
        "let calls = 0; let locale = new Intl.Locale('en', {region: {toString() { calls++; return 419n; }}}); locale.region === '419' && calls === 1",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
    let rt = Runtime::new();
    let source = "let later = false; let caught = false; try { new Intl.Locale('en', {region: 10n ** 3999n, get calendar() { later = true; return 'gregory'; }}); } catch(e) { caught = e instanceof RangeError; } caught && !later";
    let task = rt.spawn(&exe(source), &[]).unwrap();
    let mut yields = 0;
    let mut complete = false;
    for _ in 0..1000 {
        match task
            .run_with(Budget::instructions(1), &mut NoEffects)
            .unwrap()
        {
            RunOutcome::Yielded => {
                yields += 1;
                rt.collect().unwrap();
            }
            RunOutcome::Complete(value) => {
                assert_eq!(value.view(), ValueView::Bool(true));
                assert!(yields > 100, "long option formatting must yield: {yields}");
                complete = true;
                break;
            }
            other => panic!("unexpected locale option outcome: {other:?}"),
        }
    }
    assert!(complete);
}

/// Synthetic fixtures, not light-cljs output. Primitive enumeration has no
/// own keys, including when the primitive has a shared large-integer body.
#[test]
fn bigint_primitive_key_enumeration() {
    for value in ["1n", "515377520732011331036461129765621272702107522001n"] {
        let src = format!(
            "let n = {value}; let count = 0; for (let k in n) count++; count === 0 && Object.keys(n).length === 0 && Object.getOwnPropertyNames(n).length === 0"
        );
        assert_eq!(eval(&src), ValueView::Bool(true), "{src}");
    }
}

/// Synthetic fixtures adapting integer rows of QuickJS test_builtin.js.
/// Copyright (c) 2017-2021 Fabrice Bellard
/// Copyright (c) 2017-2021 Charlie Gordon
///
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
/// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
/// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
/// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
/// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
/// THE SOFTWARE.
#[test]
fn integer_typed_array_elements() {
    // Synthetic fixtures, not light-cljs output; QuickJS semantic rows are
    // recorded in docs/attribution.md.
    for src in [
        "let a = new Uint8Array(4); for (let i = 0; i < 4; i++) a[i] = i; a[0] = -1; a.length === 4 && a[0] === 255 && a[3] === 3",
        "let a = new Int8Array(1); a[0] = 255; a[0] === -1",
        "let a = new Int32Array(1); a[0] = 4294967295; a[0] === -1 && a.BYTES_PER_ELEMENT === 4 && Int32Array.BYTES_PER_ELEMENT === 4",
        "let a = new Uint8ClampedArray(4); a[0] = -100; a[1] = 1.5; a[2] = 0.5; a[3] = 1233.5; a[0] === 0 && a[1] === 2 && a[2] === 0 && a[3] === 255",
        "let a = new Uint8Array(4); a[10] = 1; a[1.5] = 3; a['-0'] = 4; a[10] === undefined && a[1.5] === undefined && a['-0'] === undefined",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
}

#[test]
fn float_typed_array_elements() {
    // Synthetic fixtures, not light-cljs output.
    for src in [
        "let a = new Float16Array(7); a[0] = 1; a[1] = 65504; a[2] = 65520; a[3] = 5.960464477539063e-8; a[4] = 0.1; a[5] = -0; a[6] = NaN; a[0] === 1 && a[1] === 65504 && a[2] === Infinity && a[3] === 5.960464477539063e-8 && a[4] === 0.0999755859375 && 1 / a[5] === -Infinity && a[6] !== a[6]",
        "let a = new Float32Array(4); a[0] = 0.1; a[1] = -0; a[2] = 1e40; a[3] = NaN; a[0] === 0.10000000149011612 && 1 / a[1] === -Infinity && a[2] === Infinity && a[3] !== a[3]",
        "let a = new Float64Array(4); a[0] = 0.1; a[1] = -0; a[2] = -Infinity; a[3] = NaN; a[0] === 0.1 && 1 / a[1] === -Infinity && a[2] === -Infinity && a[3] !== a[3]",
        "let a = new Float16Array(4); a[0] = 1 + 2**-11; a[1] = 1 + 3 * 2**-11; a[2] = 1 + 2**-11 + 2**-40; a[3] = -(2**-25); a[0] === 1 && a[1] === 1 + 2**-9 && a[2] === 1 + 2**-10 && 1 / a[3] === -Infinity",
        "let a = new Float32Array(4); a[0] = 1 + 2**-24; a[1] = 1 + 3 * 2**-24; a[2] = 2**-149; a[3] = 2**-150; a[0] === 1 && a[1] === 1 + 2**-22 && a[2] === 2**-149 && a[3] === 0",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
}

/// Synthetic fixture adapting the buffer-view rows of QuickJS test_builtin.js.
/// Copyright (c) 2017-2021 Fabrice Bellard
/// Copyright (c) 2017-2021 Charlie Gordon
///
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
/// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
/// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
/// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
/// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
/// THE SOFTWARE.
#[test]
fn float_typed_array_byte_layout() {
    assert_eq!(
        eval(
            "let buffer = new ArrayBuffer(16); let a = new Uint32Array(buffer, 12, 1); a[0] = -1; a = new Uint16Array(buffer, 2); a[0] = -1; a = new Float16Array(buffer, 8, 1); a[0] = 1; a = new Float32Array(buffer, 8, 1); a[0] = 1; let bytes = new Uint8Array(buffer); let expected = [0,0,255,255,0,0,0,0,0,0,128,63,255,255,255,255]; let ok = true; for (let i = 0; i < 16; i++) { if (bytes[i] !== expected[i]) ok = false; } ok"
        ),
        ValueView::Bool(true)
    );
    for src in [
        "let a = new Float16Array(1); a[0] = 1; let b = new Uint8Array(a.buffer); b[0] === 0 && b[1] === 60",
        "let a = new Float64Array(1); a[0] = 1; let b = new Uint8Array(a.buffer); b[0] === 0 && b[1] === 0 && b[2] === 0 && b[3] === 0 && b[4] === 0 && b[5] === 0 && b[6] === 240 && b[7] === 63",
        "let a = new Float16Array(1); a[0] = NaN; new Uint16Array(a.buffer)[0] === 32256",
        "let a = new Float32Array(1); a[0] = NaN; new Uint32Array(a.buffer)[0] === 2143289344",
        "let a = new Float64Array(1); a[0] = NaN; let b = new Uint32Array(a.buffer); b[0] === 0 && b[1] === 2146959360",
        "let b = new ArrayBuffer(8); let u = new Uint16Array(b); u[0] = 1; u[1] = 32768; u[2] = 31745; let a = new Float16Array(b); a[0] === 2**-24 && 1 / a[1] === -Infinity && a[2] !== a[2]",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
}

#[test]
fn float_typed_array_nan_encodings() {
    // Synthetic fixtures, not light-cljs output. Seed positive signaling and
    // negative payload-bearing NaNs without using an already canonical NaN.
    for (input_kind, seed) in [
        ("Float32Array", "u[0] = 0x7f800001; u[1] = 0xffc12345;"),
        (
            "Float64Array",
            "u[0] = 1; u[1] = 0x7ff00000; u[2] = 0x89abcdef; u[3] = 0xfff81234;",
        ),
    ] {
        for (output_kind, word_kind, expected) in [
            (
                "Float16Array",
                "Uint16Array",
                "w[0] === 0x7e00 && w[1] === 0x7e00",
            ),
            (
                "Float32Array",
                "Uint32Array",
                "w[0] === 0x7fc00000 && w[1] === 0x7fc00000",
            ),
            (
                "Float64Array",
                "Uint32Array",
                "w[0] === 0 && w[1] === 0x7ff80000 && w[2] === 0 && w[3] === 0x7ff80000",
            ),
        ] {
            let src = format!(
                "let input = new {input_kind}(2); let u = new Uint32Array(input.buffer); {seed} let output = new {output_kind}(2); output[0] = input[0]; output[1] = input[1]; let w = new {word_kind}(output.buffer); input[0] !== input[0] && input[1] !== input[1] && output[0] !== output[0] && output[1] !== output[1] && {expected}"
            );
            assert_eq!(eval(&src), ValueView::Bool(true), "{src}");
        }
    }
}

#[test]
fn float_typed_array_constructors_and_coercion() {
    // Synthetic fixtures, not light-cljs output.
    for (kind, size) in [
        ("Float16Array", 2),
        ("Float32Array", 4),
        ("Float64Array", 8),
    ] {
        let src = format!(
            "let a = new {kind}('2.9'); let b = new {kind}(a.buffer, {size}, 1); a[1] = {{valueOf() {{ return '0.1'; }}}}; b[0] === a[1] && a.length === 2 && b.byteOffset === {size} && b.byteLength === {size} && {kind}.BYTES_PER_ELEMENT === {size} && a.BYTES_PER_ELEMENT === {size} && {kind}.length === 3 && a instanceof {kind} && Object.getPrototypeOf({kind}) === Object.getPrototypeOf(Uint8Array) && Object.prototype.toString.call(a) === '[object {kind}]'"
        );
        assert_eq!(eval(&src), ValueView::Bool(true), "{src}");
        for (expr, error) in [
            (format!("{kind}(1)"), "TypeError"),
            (format!("new {kind}(new ArrayBuffer(16), 1)"), "RangeError"),
            (format!("new {kind}(new ArrayBuffer(17))"), "RangeError"),
            (
                format!("new {kind}(new ArrayBuffer(16), 16, 1)"),
                "RangeError",
            ),
            (format!("let a = new {kind}(1); a[0] = 1n"), "TypeError"),
            (
                format!("let a = new {kind}(1); a[0] = Symbol()"),
                "TypeError",
            ),
        ] {
            let src = format!(
                "let ok = false; try {{ {expr}; }} catch(e) {{ ok = e instanceof {error}; }} ok"
            );
            assert_eq!(eval(&src), ValueView::Bool(true), "{src}");
        }
    }
}

#[test]
fn typed_array_views_share_buffer() {
    assert_eq!(
        eval(
            "let b = new ArrayBuffer(16); let u32 = new Uint32Array(b, 12, 1); u32[0] = -1; let u16 = new Uint16Array(b, 2); u16[0] = -1; let u8 = new Uint8Array(b); u8[2] === 255 && u8[3] === 255 && u8[12] === 255 && u8[13] === 255 && u8[14] === 255 && u8[15] === 255 && u32.length === 1 && u16.length === 7 && u8.buffer === b && u32.byteOffset === 12"
        ),
        ValueView::Bool(true)
    );
    for args in ["b, 2", "b, 12, 2"] {
        let src = format!(
            "let b = new ArrayBuffer(16); let ok = false; try {{ new Uint32Array({args}); }} catch(e) {{ ok = e instanceof RangeError; }} ok"
        );
        assert_eq!(eval(&src), ValueView::Bool(true), "{src}");
    }
}

#[test]
fn typed_array_after_transfer() {
    assert_eq!(
        eval(
            "let b = new ArrayBuffer(16); let a = new Uint8Array(b); a[0] = 42; b.transfer(); a[0] === undefined && a.length === 0 && a.byteOffset === 0 && a.byteLength === 0 && a.buffer === b"
        ),
        ValueView::Bool(true)
    );
}

#[test]
fn typed_array_integer_indexed_properties() {
    for src in [
        "let a = new Uint8Array(2); a['01'] = 9; a['1.0'] = 8; a.x = 7; a['01'] === 9 && a['1.0'] === 8 && a.x === 7 && a[1] === 0",
        "let a = new Uint8Array(2); let p = { '3': 9, '-0': 7 }; Object.setPrototypeOf(a, p); a[3] === undefined && a['-0'] === undefined && !(3 in a) && !('-0' in a) && 0 in a && 1 in a",
        "let a = new Uint8Array(2); a.NaN = 8; a.Infinity = 8; a[-1] = 8; a.NaN === undefined && a.Infinity === undefined && a[-1] === undefined && !('NaN' in a)",
        "let a = new Uint8Array(2); let n = 0; let v = {valueOf() { n++; return 257; }}; let r = (a[0] = v); a[8] = v; a['-0'] = v; a[0] === 1 && n === 3 && r === v",
        "let a = new Uint8Array(1); let ok = false; try { a[9] = Symbol(); } catch(e) { ok = e instanceof TypeError; } ok",
        "let a = new Uint8Array(2); let b = a.buffer; a[0] = {valueOf() { b.transfer(); return 42; }}; a[0] === undefined && a.length === 0",
        "let a = new Uint8Array(2); let r = Object.defineProperty(a, '0', {value: {valueOf() { return 258; }}}); r === a && a[0] === 2",
        "let a = new Uint8Array(2); let b = a.buffer; let r = Object.defineProperty(a, '0', {value: {valueOf() { b.transfer(); return 258; }}}); r === a && a.length === 0",
        "let a = new Uint8Array(2); let n = 0; let ok = false; try { Object.defineProperty(a, '9', {value: {valueOf() { n++; return 2; }}}); } catch(e) { ok = e instanceof TypeError; } ok && n === 0",
        "let a = new Uint8Array(2); let d = Object.getOwnPropertyDescriptor(a, '0'); d.value === 0 && d.writable && d.enumerable && d.configurable && Object.keys(a).length === 2 && Object.keys(a)[1] === '1'",
        "let a = new Uint8Array(1); Object.defineProperty(a, '0', {value: 258}); let ok = false; try { delete a[0]; } catch(e) { ok = e instanceof TypeError; } a[0] === 2 && ok && delete a[4]",
        "let a = new Uint8Array(1); Object.preventExtensions(a); a[0] = 9; a[0] === 9 && !Object.isExtensible(a) && !Object.isFrozen(a)",
        "let a = new Uint8Array(1); let ok = false; try { Object.freeze(a); } catch(e) { ok = e instanceof TypeError; } ok && !Object.isExtensible(a)",
        "let a = new Uint8Array(0); Object.freeze(a); Object.isFrozen(a)",
        "let a = new Uint8Array(1); let ok = false; try { a[0] = 1n; } catch(e) { ok = e instanceof TypeError; } ok",
        "let a = new Uint8Array(1); let ok = false; try { a[0] = {valueOf() { return 1n; }}; } catch(e) { ok = e instanceof TypeError; } ok",
        "let a = new Uint8Array(2); a[0] = 3; JSON.stringify(a) === '{\"0\":3,\"1\":0}'",
        "let ok = false; try { JSON.stringify(new Uint8Array(4097)); } catch(e) { ok = e instanceof RangeError; } ok",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
}

#[test]
fn typed_array_for_in_elements() {
    for source in [
        "let a = new Uint8Array(2); let keys = ''; for (let k in a) keys += k; keys === '01'",
        "let a = new Uint8Array(2); let o = Object.create(a); o[0] = 7; o.x = 8; let keys = ''; for (let k in o) keys += k; keys === '0x1'",
        "let a = new Uint8Array(2); a.x = 8; let keys = ''; for (let k in a) { keys += k; if (k === '0') a.buffer.transfer(); } keys === '0x'",
        "let a = new Uint8Array(2); a.buffer.transfer(); let keys = ''; for (let k in a) keys += k; keys === ''",
    ] {
        assert_eq!(eval(source), ValueView::Bool(true), "{source}");
    }
}

#[test]
fn typed_array_generic_push_coercion() {
    for source in [
        "let a = new Uint8Array(1); Object.defineProperty(a, 'length', {value: 0, writable: true}); let n = 0; let r = Array.prototype.push.call(a, {valueOf() { n++; return 257; }}); r === 1 && a[0] === 1 && a.length === 1 && n === 1",
        "let a = new Uint8Array(1); let n = 0; let ok = false; try { Array.prototype.push.call(a, {valueOf() { n++; return 257; }}); } catch(e) { ok = e instanceof TypeError; } ok && n === 1 && a[1] === undefined",
        "let a = new Uint8Array(1); Object.defineProperty(a, 'length', {value: 0, writable: true}); let b = a.buffer; let r = Array.prototype.push.call(a, {valueOf() { b.transfer(); return 257; }}); r === 1 && a.length === 1 && a[0] === undefined",
        "let a = new Uint8Array(1); Object.defineProperty(a, 'length', {value: 0, writable: true}); let ok = false; try { Array.prototype.push.call(a, {valueOf() { throw 42; }}); } catch(e) { ok = e === 42; } ok && a.length === 0 && a[0] === 0",
    ] {
        assert_eq!(eval(source), ValueView::Bool(true), "{source}");
    }
}

#[test]
fn typed_array_element_descriptor_restrictions() {
    for descriptor in [
        "{writable: false}",
        "{enumerable: false}",
        "{configurable: false}",
        "{get: undefined}",
        "{set: function(v) {}}",
        "{writable: false, value: {valueOf() { n++; return 5; }}}",
        "{enumerable: false, value: {valueOf() { n++; return 5; }}}",
        "{configurable: false, value: {valueOf() { n++; return 5; }}}",
    ] {
        let source = format!(
            "let a = new Uint8Array(1); a[0] = 9; let n = 0; let ok = false; try {{ Object.defineProperty(a, '0', {descriptor}); }} catch(e) {{ ok = e instanceof TypeError; }} ok && a[0] === 9 && n === 0"
        );
        assert_eq!(eval(&source), ValueView::Bool(true), "{source}");
    }
    assert_eq!(
        eval(
            "let a = new Uint8Array(1); Object.defineProperty(a, '0', {writable: true, enumerable: true, configurable: true, value: 258}); a[0] === 2"
        ),
        ValueView::Bool(true)
    );
}

#[test]
fn typed_array_as_prototype_stops_integer_index_writes() {
    for source in [
        "let a = new Uint8Array(1); let n = 0; let p = {}; Object.defineProperty(p, '0', {set: function(v) { n++; }}); Object.setPrototypeOf(a, p); let o = Object.create(a); o[0] = 256; o[0] === 256 && a[0] === 0 && n === 0",
        "let a = new Uint8Array(1); let o = Object.create(a); let n = 0; o[9] = {valueOf() { n++; return 7; }}; !(9 in o) && Object.getOwnPropertyDescriptor(o, '9') === undefined && n === 0",
        "let a = new Uint8Array(1); let o = Object.create(a); a.buffer.transfer(); o[0] = 7; !(0 in o) && Object.getOwnPropertyDescriptor(o, '0') === undefined",
        "let a = new Uint8Array(1); let o = Object.create(a); let value = {}; o[0] = value; o[0] === value && a[0] === 0",
    ] {
        assert_eq!(eval(source), ValueView::Bool(true), "{source}");
    }
}

#[test]
fn typed_array_constructor_validation_and_metadata() {
    for src in [
        "new Uint8Array().length === 0 && new Uint16Array('3.9').byteLength === 6 && new Uint32Array(-0.9).length === 0",
        "let a = new Int16Array(1); a[0] = 65535; let b = new Uint16Array(1); b[0] = -2; let c = new Uint32Array(1); c[0] = -2; a[0] === -1 && b[0] === 65534 && c[0] === 4294967294",
        "let a = new Uint8Array(4); a[0] = NaN; a[1] = Infinity; a[2] = -Infinity; a[3] = -257.9; a[0] === 0 && a[1] === 0 && a[2] === 0 && a[3] === 255",
        "let a = new Uint8ClampedArray(4); a[0] = NaN; a[1] = Infinity; a[2] = 2.5; a[3] = 3.5; a[0] === 0 && a[1] === 255 && a[2] === 2 && a[3] === 4",
        "let b = new ArrayBuffer(16); let a = new Uint32Array(b, 16, 0); a.length === 0 && a.byteOffset === 16 && a.byteLength === 0",
        "let c = Object.getPrototypeOf(Uint8Array); c === Object.getPrototypeOf(Int32Array) && c.name === 'TypedArray' && Object.getPrototypeOf(Uint8Array.prototype) === c.prototype && Uint8Array.prototype.constructor === Uint8Array && Uint8Array.length === 3",
        "let d = Object.getOwnPropertyDescriptor(Uint16Array.prototype, 'BYTES_PER_ELEMENT'); d.value === 2 && !d.writable && !d.enumerable && !d.configurable",
        "let a = new Uint8Array(1); a instanceof Uint8Array && a instanceof Object && !Array.isArray(a) && Object.prototype.toString.call(a) === '[object Uint8Array]'",
        "let b = new ArrayBuffer(8); let log = ''; let a = new Uint16Array(b, {valueOf() { log += 'o'; return 2; }}, {valueOf() { log += 'l'; return 2; }}); log === 'ol' && a.length === 2 && a.byteOffset === 2",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
    for (src, kind) in [
        ("Uint8Array(1)", "TypeError"),
        ("Object.getPrototypeOf(Uint8Array)()", "TypeError"),
        (
            "let T = Object.getPrototypeOf(Uint8Array); new T()",
            "TypeError",
        ),
        ("Uint8Array.prototype.length", "TypeError"),
        ("new Uint8Array(-1)", "RangeError"),
        ("new Int32Array(4194305)", "RangeError"),
        ("new Uint8Array(Infinity)", "RangeError"),
        ("new Uint8Array(Symbol())", "TypeError"),
        ("new Uint16Array(new ArrayBuffer(3))", "RangeError"),
        (
            "let b = new ArrayBuffer(4); b.transfer(); new Uint8Array(b)",
            "TypeError",
        ),
        (
            "let b = new ArrayBuffer(4); new Uint16Array(b, {valueOf() { b.transfer(); return 0; }})",
            "TypeError",
        ),
        (
            "let b = new ArrayBuffer(4); new Uint16Array(b, 0, {valueOf() { b.transfer(); return 1; }})",
            "TypeError",
        ),
    ] {
        let source =
            format!("let ok = false; try {{ {src}; }} catch(e) {{ ok = e instanceof {kind}; }} ok");
        assert_eq!(eval(&source), ValueView::Bool(true), "{source}");
    }
}

#[test]
fn typed_array_coercion_survives_waits_gc_and_migration() {
    // Synthetic fixtures, not light-cljs output.
    for source in [
        "let a = [0,2]; Object.defineProperty(a,'0',{get() {a.length=1; a=null; __rt.need(166,1); return 7;}}); new Uint8Array(a).join() === '7'",
        "let s = {get length() { __rt.need(166,1); return 2; },0:7,1:8}; new Uint8Array(s).join() === '7,8'",
        "let s = {length:{valueOf() { __rt.need(166,1); return 1; }},get 0() {return {valueOf() {return 7;}};}}; new Uint8Array(s).join() === '7'",
        "let a = new Uint8Array({length:1,get 0() { __rt.need(166,1); return {valueOf() {return 7;}};}}); a.join() === '7'",
        "let a = new Uint8Array([{valueOf() { __rt.need(166,1); return 7; }}]); a.join() === '7'",
        "let s = {}; Object.defineProperty(s,Symbol.iterator,{get() { s=null; __rt.need(166,1); return function () {let n=0; return {get next() {return function () {n++;return {done:n>1,value:7};};}};}; }}); new Uint8Array(s).join() === '7'",
        "let s = {}; s[Symbol.iterator] = function () {let n=0; return {get next() { __rt.need(166,1); return function () {n++;return {done:n>1,value:7};};}};}; new Uint8Array(s).join() === '7'",
        "let s = {}; s[Symbol.iterator] = function () {let n=0; return {next() {n++; if(n===1) __rt.need(166,1); return {done:n>1,value:7};}};}; new Uint8Array(s).join() === '7'",
        "let s = {}; s[Symbol.iterator] = function () {let n=0; return {next() {n++; return {get done() {if(n===1) __rt.need(166,1); return n>1;},value:7};}};}; new Uint8Array(s).join() === '7'",
        "let s = {}; s[Symbol.iterator] = function () {let n=0; return {next() {n++; return {done:n>1,get value() { __rt.need(166,1); return 7;}};}};}; new Uint8Array(s).join() === '7'",
        "let a = new Uint8Array(1); a.set({get length() { __rt.need(166,1); return 1;},0:7}); a.join() === '7'",
        "let a = new Uint8Array(1); a.set({length:1,get 0() { __rt.need(166,1); return 7;}}); a.join() === '7'",
        "let a = new Uint8Array(1); a.set([{valueOf() { __rt.need(166,1); return 7; }}]); a.join() === '7'",
        "let a = new Uint8Array(2); a.set([7],{valueOf() { __rt.need(166,1); return 1; }}); a.join() === '0,7'",
        "let a = new Uint8Array(2); a.fill({valueOf() { __rt.need(166,1); return 7; }}); a.join() === '7,7'",
        "let a = new Uint8Array(2); a.fill(7,{valueOf() { __rt.need(166,1); return 1; }}); a.join() === '0,7'",
        "let a = new Uint8Array(2); a.fill(7,0,{valueOf() { __rt.need(166,1); return 1; }}); a.join() === '7,0'",
        "let a = new Uint8Array([7,8]); a.join({toString() {a=null; __rt.need(166,1); return '-';}}) === '7-8'",
        "let a = new Float16Array(1); a[0] = {valueOf() { __rt.need(166, 1); return 0.1; }}; a[0] === 0.0999755859375",
        "let a = new Float32Array(1); a[0] = {valueOf() { __rt.need(166, 1); return 0.1; }}; a[0] === 0.10000000149011612",
        "let a = new Float64Array(1); a[0] = {valueOf() { __rt.need(166, 1); return -0; }}; 1 / a[0] === -Infinity",
        "let b = new ArrayBuffer(8); let a = new Uint16Array(b, {valueOf() { b = null; __rt.need(166, 1); return 2; }}, 2); a.length === 2 && a.byteOffset === 2 && a.buffer.byteLength === 8",
        "let b = new ArrayBuffer(8); let a = new Uint16Array(b, 0, {valueOf() { b = null; __rt.need(166, 1); return 2; }}); a.length === 2 && a.byteLength === 4",
        "let a = new Uint8Array(1); let v = {valueOf() { __rt.need(166, 1); return 257; }}; let r = (a[0] = v); a[0] === 1 && r === v",
        "let a = new Uint8Array(1); let v = {valueOf() { a = null; __rt.need(166, 1); return 257; }}; let r = (a[9] = v); a === null && r === v",
        "let log = ''; let a = new Uint8Array(1); a[{toString() { log += 'k'; __rt.need(166, 1); return '0'; }}] = {valueOf() { log += 'v'; return 4; }}; a[0] === 4 && log === 'kv'",
        "let a = new Uint8Array(1); let b = a.buffer; let n = 0; b.transfer(); a[0] = {valueOf() { __rt.need(166, 1); n++; return 42; }}; a[0] === undefined && n === 1",
        "let a = new Uint8Array(1); let r = Object.defineProperty(a, '0', {value: {valueOf() { __rt.need(166, 1); return 257; }}}); r === a && a[0] === 1",
        "let a = new Uint8Array(1); let b = a.buffer; Object.defineProperty(a, 'length', {value: 0, writable: true}); let r = Array.prototype.push.call(a, {valueOf() { a = null; __rt.need(166, 1); return 257; }}); let u = new Uint8Array(b); a === null && r === 1 && u[0] === 1",
    ] {
        let rt = Runtime::new();
        let mut task = rt.spawn(&exe(source), &[]).unwrap();
        let mut waits = 0;
        let mut done = false;
        for _ in 0..1000 {
            let (next, outcome) = std::thread::spawn(move || {
                let outcome = task
                    .run_with(Budget::instructions(1), &mut NoEffects)
                    .unwrap();
                (task, outcome)
            })
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
                    let value = rt.root(Primitive::Number(0.0));
                    task = rt
                        .complete(request.key(), request.producer(), Completion::Ready(&value))
                        .unwrap()
                        .into_runnable()
                        .pop()
                        .unwrap();
                }
                RunOutcome::Complete(value) => {
                    assert_eq!(value.view(), ValueView::Bool(true), "{source}");
                    done = true;
                    break;
                }
                other => panic!("{source}: {other:?}"),
            }
        }
        assert!(done, "instruction bound exceeded: {source}");
        assert_eq!(waits, 1);
    }
}

#[test]
fn typed_array_publication_and_foreign_access_rejected() {
    let rt = Runtime::new();
    let task = rt.spawn(&exe("new Uint8Array(8)"), &[]).unwrap();
    let RunOutcome::Complete(array) = task.run().unwrap() else {
        panic!("expected typed array");
    };
    assert!(matches!(
        rt.publish(&array),
        Err(clj_quickjs::PublishError::NotShareable { .. })
    ));
    assert!(rt.new_atom(&array).is_err());
    for body in [
        "return a[0];",
        "a[0] = 1;",
        "return a.length;",
        "return Object.keys(a);",
        "return Object.getPrototypeOf(a);",
        "Object.defineProperty(a, '0', {value: 1});",
    ] {
        let code = compile_function(&format!("function f(a) {{ {body} }}")).unwrap();
        let other = rt.spawn(&code, &[&array]).unwrap();
        let RunOutcome::Failed(error) = other.run().unwrap() else {
            panic!("expected ownership failure: {body}");
        };
        assert!(
            matches!(error.kind(), VmErrorKind::ForeignObject { .. }),
            "{body}: {error:?}"
        );
    }
}

#[test]
fn array_buffer_basics() {
    for src in [
        "new ArrayBuffer(16).byteLength === 16 && new ArrayBuffer(0).byteLength === 0",
        "let b = new ArrayBuffer(4); b.detached === false && b instanceof ArrayBuffer && Object.getPrototypeOf(b) === ArrayBuffer.prototype",
        "new ArrayBuffer().byteLength === 0 && new ArrayBuffer(3.9).byteLength === 3 && new ArrayBuffer(-0.9).byteLength === 0",
        "new ArrayBuffer('4').byteLength === 4 && new ArrayBuffer(NaN).byteLength === 0",
        "new ArrayBuffer(null).byteLength === 0 && new ArrayBuffer(true).byteLength === 1 && new ArrayBuffer(-0).byteLength === 0",
        "let b = new ArrayBuffer(3); Object.freeze(b); Object.isFrozen(b) && b.transfer().byteLength === 3 && b.detached",
        "Object.prototype.toString.call(new ArrayBuffer(2)) === '[object ArrayBuffer]'",
        "let b = new ArrayBuffer(4); b.x = 42; b.x === 42 && Object.keys(b)[0] === 'x'",
        "let d = Object.getOwnPropertyDescriptor(ArrayBuffer.prototype, 'byteLength'); typeof d.get === 'function' && d.set === undefined && !d.enumerable && d.configurable",
        "let n = 0; new ArrayBuffer({valueOf() { n++; return 5; }}).byteLength === 5 && n === 1",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
    for (src, kind) in [
        ("new ArrayBuffer(-1)", "RangeError"),
        ("new ArrayBuffer(Infinity)", "RangeError"),
        ("new ArrayBuffer(9007199254740992)", "RangeError"),
        ("new ArrayBuffer(16777217)", "RangeError"),
        ("ArrayBuffer(8)", "TypeError"),
        ("new ArrayBuffer(Symbol())", "TypeError"),
        ("ArrayBuffer.prototype.byteLength", "TypeError"),
    ] {
        let expected = if kind == "RangeError" {
            VmErrorKind::RangeError
        } else {
            VmErrorKind::TypeError
        };
        assert_eq!(failure(src).kind(), expected, "{src}");
        let caught = format!(
            "let ok = false; try {{ {src}; }} catch (e) {{ ok = e instanceof {kind}; }} ok"
        );
        assert_eq!(eval(&caught), ValueView::Bool(true), "{src}");
    }
}

#[test]
fn array_buffer_intrinsic_metadata() {
    for source in [
        "ArrayBuffer.name === 'ArrayBuffer' && ArrayBuffer.length === 1 && ArrayBuffer.prototype.constructor === ArrayBuffer",
        "ArrayBuffer.prototype.transfer.name === 'transfer' && ArrayBuffer.prototype.transfer.length === 0 && ArrayBuffer.prototype.slice.name === 'slice' && ArrayBuffer.prototype.slice.length === 2",
        "let d = Object.getOwnPropertyDescriptor(ArrayBuffer.prototype, 'detached'); d.get.name === 'get detached' && d.get.length === 0 && d.set === undefined && !d.enumerable && d.configurable",
        "let d = Object.getOwnPropertyDescriptor(ArrayBuffer.prototype, 'byteLength'); d.get.name === 'get byteLength' && d.get.length === 0 && d.set === undefined && !d.enumerable && d.configurable",
        "let d = Object.getOwnPropertyDescriptor(ArrayBuffer.prototype, 'transfer'); d.writable && !d.enumerable && d.configurable",
    ] {
        assert_eq!(eval(source), ValueView::Bool(true), "{source}");
    }
}

#[test]
fn array_buffer_rejects_resizable_options() {
    for options in [
        "{}",
        "{maxByteLength: 8}",
        "{get maxByteLength() { throw 42; }}",
    ] {
        let source = format!("new ArrayBuffer(4, {options})");
        assert_eq!(
            failure(&source).kind(),
            VmErrorKind::UnsupportedOperand,
            "{source}"
        );
    }
    assert_eq!(
        eval(
            "let caught = false; try { new ArrayBuffer({valueOf() { throw 42; }}, {}); } catch(e) { caught = e === 42; } caught"
        ),
        ValueView::Bool(true)
    );
    assert_eq!(
        failure("new ArrayBuffer(-1, {})").kind(),
        VmErrorKind::RangeError
    );
    assert_eq!(
        failure("ArrayBuffer(4, {maxByteLength: 8})").kind(),
        VmErrorKind::TypeError
    );
    for options in ["undefined", "null", "true", "'ignored'"] {
        assert_eq!(
            eval(&format!("new ArrayBuffer(4, {options}).byteLength === 4")),
            ValueView::Bool(true)
        );
    }
}

/// Synthetic fixtures, not light-cljs output.
#[test]
fn array_buffer_transfer_detaches() {
    for src in [
        "let b = new ArrayBuffer(16); let c = b.transfer(); b.byteLength === 0 && b.detached === true && c.byteLength === 16 && c !== b && !c.detached",
        "new ArrayBuffer(4).transfer(8).byteLength === 8 && new ArrayBuffer(4).transfer(2).byteLength === 2",
        "let b = new ArrayBuffer(0); let c = b.transfer(); b.detached && !c.detached && c.byteLength === 0",
        "let b = new ArrayBuffer(2); b.transfer(); let ok = false; try { b.transfer(); } catch (e) { ok = e instanceof TypeError; } ok",
        "let b = new ArrayBuffer(2); try { b.transfer(-1); } catch (e) {} !b.detached && b.byteLength === 2",
        "let b = new ArrayBuffer(2); let c = b.transfer({valueOf() { return 8; }}); b.detached && c.byteLength === 8",
        "let b = new ArrayBuffer(2); b.x = 7; b.transfer(undefined).byteLength === 2 && b.x === 7",
        "let b = new ArrayBuffer(2); b.transfer({valueOf() { return undefined; }}).byteLength === 0 && b.detached",
        "let b = new ArrayBuffer(2); let caught = false; try { b.transfer({valueOf() { b.transfer(); return 4; }}); } catch (e) { caught = true; } caught && b.detached",
        "let b = new ArrayBuffer(2); let caught = false; try { b.transfer({valueOf() { throw 42; }}); } catch (e) { caught = e === 42; } caught && !b.detached && b.byteLength === 2",
        "let n = 0; let caught = false; try { ArrayBuffer.prototype.transfer.call({}, {valueOf() { n++; return 4; }}); } catch (e) { caught = true; } caught && n === 0",
        "let b = new ArrayBuffer(2); let c = b.slice(1); b.byteLength === 2 && !b.detached && c.byteLength === 1 && c !== b",
        "new ArrayBuffer(8).slice(-3, -1).byteLength === 2 && new ArrayBuffer(8).slice(6, 2).byteLength === 0",
        "new ArrayBuffer(8).slice(-Infinity, Infinity).byteLength === 8 && new ArrayBuffer(8).slice(Infinity).byteLength === 0",
        "let b = new ArrayBuffer(8); b.slice(0, {valueOf() { return undefined; }}).byteLength === 0 && b.slice(0, undefined).byteLength === 8",
        "let b = new ArrayBuffer(8); let caught = false; try { b.slice({valueOf() { b.transfer(); return 1; }}); } catch (e) { caught = true; } caught && b.detached",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
    assert_eq!(
        failure("let b = new ArrayBuffer(2); b.transfer(); b.transfer()").kind(),
        VmErrorKind::TypeError
    );
}

/// Synthetic fixtures, not light-cljs output.
#[test]
fn array_buffer_slice_truncates_indices_before_relative_clamping() {
    for (arguments, expected_length) in [
        ("-0.5", 8.0),
        ("0, -0.5", 0.0),
        ("-0.9, -0.1", 0.0),
        ("0.5, 7.5", 7.0),
        ("-3.9, -1.1", 2.0),
        ("-0", 8.0),
        ("0, -0", 0.0),
        ("NaN", 8.0),
        ("0, NaN", 0.0),
    ] {
        let source = format!("new ArrayBuffer(8).slice({arguments}).byteLength");
        assert_eq!(
            eval(&source),
            ValueView::Number(expected_length),
            "{source}"
        );
    }
}

#[test]
fn array_buffer_coercion_survives_waits_gc_and_migration() {
    for source in [
        "new ArrayBuffer({valueOf() { __rt.need(165, 1); return 4; }}).byteLength === 4",
        "let b = new ArrayBuffer(2); let c = b.transfer({valueOf() { __rt.need(165, 1); return 4; }}); b.detached && c.byteLength === 4",
        "let b = new ArrayBuffer(2); let c = b.transfer({valueOf() { b = null; __rt.need(165, 1); return 4; }}); b === null && c.byteLength === 4",
        "let b = new ArrayBuffer(8); b.slice({valueOf() { __rt.need(165, 1); return 2; }}, 4).byteLength === 2 && !b.detached",
    ] {
        let rt = Runtime::new();
        let mut task = rt.spawn(&exe(source), &[]).unwrap();
        let mut waits = 0;
        let mut complete = false;
        for _ in 0..1000 {
            let (next, outcome) = std::thread::spawn(move || {
                let outcome = task
                    .run_with(Budget::instructions(1), &mut NoEffects)
                    .unwrap();
                (task, outcome)
            })
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
                    let value = rt.root(Primitive::Number(0.0));
                    task = rt
                        .complete(request.key(), request.producer(), Completion::Ready(&value))
                        .unwrap()
                        .into_runnable()
                        .pop()
                        .unwrap();
                }
                RunOutcome::Complete(value) => {
                    assert_eq!(value.view(), ValueView::Bool(true), "{source}");
                    complete = true;
                    break;
                }
                other => panic!("{source}: {other:?}"),
            }
        }
        assert!(complete, "instruction bound exceeded: {source}");
        assert_eq!(waits, 1);
    }
}

#[test]
fn array_buffer_publication_rejected() {
    let rt = Runtime::new();
    let task = rt.spawn(&exe("new ArrayBuffer(8)"), &[]).unwrap();
    let RunOutcome::Complete(buffer) = task.run().unwrap() else {
        panic!("expected buffer")
    };
    assert!(matches!(
        rt.publish(&buffer),
        Err(clj_quickjs::PublishError::NotShareable { .. })
    ));
    assert!(rt.new_atom(&buffer).is_err());
    for source in [
        "function f(b) { return b.byteLength; }",
        "function f(b) { return ArrayBuffer.prototype.transfer.call(b); }",
    ] {
        let foreign = rt
            .spawn(&compile_function(source).unwrap(), &[&buffer])
            .unwrap();
        assert!(matches!(foreign.run().unwrap(), RunOutcome::Failed(e)
            if matches!(e.kind(), VmErrorKind::ForeignObject { .. })));
    }
}

/// Synthetic fixtures, not light-cljs output. Date.UTC rows adapted from
/// QuickJS tests/test_builtin.js::test_date, revision 7d1c13c5d8f8.
/// Copyright (c) 2017-2021 Fabrice Bellard and Charlie Gordon.
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
fn date_utc_and_time_value() {
    for (args, expected) in [
        ("2017", 1483228800000.0),
        ("2017, 9", 1506816000000.0),
        ("2017, 9, 22", 1508630400000.0),
        ("2017, 9, 22, 18", 1508695200000.0),
        ("2017, 9, 22, 18, 10", 1508695800000.0),
        ("2017, 9, 22, 18, 10, 11", 1508695811000.0),
        ("2017, 9, 22, 18, 10, 11, 91", 1508695811091.0),
        ("2017, 9, 22, 18, 10, 11, 91, NaN", 1508695811091.0),
        (
            "1970, 0, 1, 80063993375, 29, 1, -288230376151711740",
            29312.0,
        ),
        (
            "1970, 0, 213503982336, 0, 0, 0, -18446744073709552000",
            34447360.0,
        ),
        ("2017, 9, 22 - 1e10, 18 + 24e10", 1508695200000.0),
        ("2017, 9, 22, 18 - 1e10, 10 + 60e10", 1508695800000.0),
        ("2017, 9, 22, 18, 10 - 1e10, 11 + 60e10", 1508695811000.0),
        (
            "2017, 9, 22, 18, 10, 11 - 1e12, 91 + 1000e12",
            1508695811091.0,
        ),
    ] {
        let src = format!("Date.UTC({args})");
        assert_eq!(eval(&src), ValueView::Number(expected), "{src}");
    }
    for args in [
        "",
        "NaN",
        "2017, NaN",
        "2017, 9, NaN",
        "2017, 9, 22, NaN",
        "2017, 9, 22, 18, NaN",
        "2017, 9, 22, 18, 10, NaN",
        "2017, 9, 22, 18, 10, 11, NaN",
    ] {
        let src = format!("Number.isNaN(Date.UTC({args}))");
        assert_eq!(eval(&src), ValueView::Bool(true), "{src}");
    }
    for src in [
        "new Date(1506098258091).getTime() === 1506098258091",
        "Number.isNaN(new Date(8.64e15 + 1).getTime())",
        "let d = new Date(0); d.setTime(5) === 5 && d.valueOf() === 5",
        "let caught = false; try { Date.prototype.getTime.call({}); } catch (e) { caught = e instanceof TypeError; } caught",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
}

/// Synthetic fixtures, not light-cljs output: exact huge month balancing.
#[test]
fn date_utc_huge_months() {
    for (args, expected) in [
        ("-6004799503160663, 72057594037927936", -62219923200000.0),
        ("-6004799503160663, 72057594037927952", -62177760000000.0),
        ("-6004799503160663, 72057594037927968", -62135596800000.0),
        ("6004799503160663, -72057594037927936", -62114601600000.0),
        ("6004799503160663, -72057594037927952", -62156764800000.0),
        ("6004799503160663, -72057594037927968", -62198755200000.0),
        (
            "-768614336404564608, 9223372036854774784",
            -62156764800000.0,
        ),
        (
            "-768614336404564608, 9223372036854775808",
            -62146137600000.0,
        ),
        (
            "768614336404564608, -9223372036854774784",
            -62146137600000.0,
        ),
        (
            "768614336404564608, -9223372036854775808",
            -62156764800000.0,
        ),
        (
            "-768614336404564864, 9223372036854777856",
            -62156764800000.0,
        ),
        (
            "768614336404564864, -9223372036854777856",
            -62146137600000.0,
        ),
    ] {
        let src = format!("Date.UTC({args})");
        assert_eq!(eval(&src), ValueView::Number(expected), "{src}");
    }
    for exponent in [53, 63, 127, 900, 1020] {
        for sign in ["", "-"] {
            let src = format!("let y = {sign}(2 ** {exponent}); Date.UTC(-y, 12 * y)");
            assert_eq!(eval(&src), ValueView::Number(-62167219200000.0), "{src}");
        }
    }
}

/// Synthetic fixtures, not light-cljs output: one rounding of the calendar day.
#[test]
fn date_utc_huge_year_cancellation() {
    for args in [
        "35184372088833, 0, -12850828021936060",
        "35184372088833, 2, -12850828021936120",
        "-35184372088833, 0, 12850828023375116",
        "-35184372088833, 2, 12850828023375056",
    ] {
        let src = format!("Date.UTC({args})");
        assert_eq!(eval(&src), ValueView::Number(-86400000.0), "{src}");
    }
    for year in [
        "2 ** 126",
        "-(2 ** 126)",
        "2 ** 127",
        "-(2 ** 127)",
        "1e308",
        "-1e308",
    ] {
        let src = format!("Number.isNaN(Date.UTC({year}, 0))");
        assert_eq!(eval(&src), ValueView::Bool(true), "{src}");
    }
}

/// Synthetic fixtures, not light-cljs output: UTC ISO formatting and expanded
/// years. The 2017 row is adapted from QuickJS tests/test_builtin.js::test_date,
/// revision 7d1c13c5d8f8. Copyright (c) 2017-2021 Fabrice Bellard
/// and Charlie Gordon.
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
fn date_to_iso_string() {
    for (time, expected) in [
        ("1506098258091", "2017-09-22T16:37:38.091Z"),
        ("0", "1970-01-01T00:00:00.000Z"),
        ("-1", "1969-12-31T23:59:59.999Z"),
        ("8.64e15", "+275760-09-13T00:00:00.000Z"),
        ("-8.64e15", "-271821-04-20T00:00:00.000Z"),
        ("-62198755200000", "-000001-01-01T00:00:00.000Z"),
        ("253402300800000", "+010000-01-01T00:00:00.000Z"),
        ("-62167219200000", "0000-01-01T00:00:00.000Z"),
        ("-62135596800000", "0001-01-01T00:00:00.000Z"),
        ("253402300799999", "9999-12-31T23:59:59.999Z"),
        (
            "Date.UTC(2000, 1, 29, 12, 34, 56, 7)",
            "2000-02-29T12:34:56.007Z",
        ),
        ("Date.UTC(1900, 2, 1)", "1900-03-01T00:00:00.000Z"),
        ("-0.9", "1970-01-01T00:00:00.000Z"),
    ] {
        let src = format!("new Date({time}).toISOString() === '{expected}'");
        assert_eq!(eval(&src), ValueView::Bool(true), "{src}");
    }
    for time in [
        "NaN",
        "Infinity",
        "-Infinity",
        "8.64e15 + 1",
        "-8.64e15 - 1",
    ] {
        let src = format!(
            "let caught = false; try {{ new Date({time}).toISOString(); }} catch(e) {{ caught = e instanceof RangeError && e.message === 'Invalid time value'; }} caught"
        );
        assert_eq!(eval(&src), ValueView::Bool(true), "{src}");
    }
}

/// Synthetic fixtures, not light-cljs output: QuickJS's non-locale formats.
#[test]
fn date_to_string_formats() {
    use clj_quickjs::host::{TimeZone, sim::SimTimeHost};

    for (offset, time, date, clock, utc) in [
        (
            3600,
            946681200000.0,
            "Sat Jan 01 2000",
            "00:00:00 GMT+0100",
            "Fri, 31 Dec 1999 23:00:00 GMT",
        ),
        (
            19800,
            946684800000.0,
            "Sat Jan 01 2000",
            "05:30:00 GMT+0530",
            "Sat, 01 Jan 2000 00:00:00 GMT",
        ),
        (
            -7200,
            946684800000.0,
            "Fri Dec 31 1999",
            "22:00:00 GMT-0200",
            "Sat, 01 Jan 2000 00:00:00 GMT",
        ),
        (
            0,
            946684800000.0,
            "Sat Jan 01 2000",
            "00:00:00 GMT+0000",
            "Sat, 01 Jan 2000 00:00:00 GMT",
        ),
        // QuickJS truncates its offset to whole minutes before decomposition.
        (
            61,
            946684800000.0,
            "Sat Jan 01 2000",
            "00:01:00 GMT+0001",
            "Sat, 01 Jan 2000 00:00:00 GMT",
        ),
        (
            -61,
            946684800000.0,
            "Fri Dec 31 1999",
            "23:59:00 GMT-0001",
            "Sat, 01 Jan 2000 00:00:00 GMT",
        ),
    ] {
        let runtime = Runtime::new();
        let mut host = SimTimeHost::new(time, TimeZone::fixed_offset_seconds(offset).unwrap());
        let src = format!(
            "let d = new Date({time}); d.toString() === '{date} {clock}' && d.toDateString() === '{date}' && d.toTimeString() === '{clock}' && d.toUTCString() === '{utc}' && d.toGMTString() === '{utc}' && Date.prototype.toGMTString === Date.prototype.toUTCString && typeof Date() === 'string' && Date() === '{date} {clock}' && Date(Symbol(), {{valueOf() {{throw 1;}}}}) === '{date} {clock}'"
        );
        let task = runtime
            .spawn(&compile_script("date-formats.js", &src).unwrap(), &[])
            .unwrap();
        let mut result = None;
        for _ in 0..10000 {
            match task.run_with(Budget::instructions(1), &mut host).unwrap() {
                RunOutcome::Complete(value) => {
                    result = Some(value);
                    break;
                }
                RunOutcome::Yielded => {
                    runtime.request_collection().unwrap();
                }
                other => panic!("{src}: {other:?}"),
            }
        }
        let value = result.expect("bounded Date formatting completes");
        assert_eq!(
            runtime.scope(|s| s.view(s.local(&value).unwrap())),
            ValueView::Bool(true),
            "{src}"
        );
        assert_eq!(host.clock_reads(), 3);
        assert!(task.dependency_log().edges().is_empty());
    }
    for src in [
        "new Date(-62198755200000).toString() === 'Fri Jan 01 -0001 00:00:00 GMT+0000' && new Date(-62198755200000).toUTCString() === 'Fri, 01 Jan -0001 00:00:00 GMT'",
        "new Date(-62167219200000).toDateString() === 'Sat Jan 01 0000' && new Date(253402300800000).toDateString() === 'Sat Jan 01 10000'",
        "new Date(8.64e15).toString() === 'Sat Sep 13 275760 00:00:00 GMT+0000' && new Date(-8.64e15).toString() === 'Tue Apr 20 -271821 00:00:00 GMT+0000'",
        "new Date(-1).toUTCString() === 'Wed, 31 Dec 1969 23:59:59 GMT' && String(new Date(0)) === 'Thu Jan 01 1970 00:00:00 GMT+0000'",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
}

/// Synthetic fixtures, not light-cljs output: branding, NaN, and descriptors.
#[test]
fn date_string_format_receivers_and_descriptors() {
    for name in [
        "toString",
        "toDateString",
        "toTimeString",
        "toUTCString",
        "toGMTString",
    ] {
        let function_name = if name == "toGMTString" {
            "toUTCString"
        } else {
            name
        };
        let src = format!(
            "let m = Date.prototype.{name}; let p = Object.getOwnPropertyDescriptor(Date.prototype, '{name}'); m.name === '{function_name}' && m.length === 0 && p.value === m && p.writable && !p.enumerable && p.configurable && new Date(NaN).{name}() === 'Invalid Date' && new Date(Infinity).{name}() === 'Invalid Date'"
        );
        assert_eq!(eval(&src), ValueView::Bool(true), "{src}");
        for receiver in [
            "{}",
            "null",
            "undefined",
            "1",
            "'0'",
            "Date.prototype",
            "Object.create(new Date(0))",
        ] {
            let src = format!(
                "let caught = false; try {{Date.prototype.{name}.call({receiver});}} catch(e) {{caught = e instanceof TypeError;}} caught"
            );
            assert_eq!(eval(&src), ValueView::Bool(true), "{src}");
        }
        let src = format!(
            "let d = new Date(NaN); d.valueOf = function() {{throw 1;}}; Object.setPrototypeOf(d, null); Date.prototype.{name}.call(d, {{valueOf() {{throw 2;}}}}) === 'Invalid Date'"
        );
        assert_eq!(eval(&src), ValueView::Bool(true), "{src}");
        let src = format!(
            "let caught = false; try {{new Date.prototype.{name}();}} catch(e) {{caught = e instanceof TypeError;}} caught"
        );
        assert_eq!(eval(&src), ValueView::Bool(true), "{src}");
    }
}

/// Synthetic fixtures, not light-cljs output: brand checks and method metadata.
#[test]
fn date_to_iso_string_receiver_and_descriptor() {
    for src in [
        "Date.prototype.toISOString.name === 'toISOString' && Date.prototype.toISOString.length === 0",
        "let d = Object.getOwnPropertyDescriptor(Date.prototype, 'toISOString'); d.value === Date.prototype.toISOString && d.writable && !d.enumerable && d.configurable",
        "let d = new Date(0); d.valueOf = function() {throw 1;}; d.toString = function() {throw 2;}; d.toISOString({valueOf() {throw 3;}}) === '1970-01-01T00:00:00.000Z'",
        "let d = new Date(-1); Object.setPrototypeOf(d, null); Date.prototype.toISOString.call(d) === '1969-12-31T23:59:59.999Z'",
        "let d = new Date(NaN); d.setTime(0); Object.freeze(d); d.toISOString() === '1970-01-01T00:00:00.000Z'",
        "let caught = false; try { new Date.prototype.toISOString(); } catch(e) { caught = e instanceof TypeError; } caught",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
    for receiver in [
        "{}",
        "null",
        "undefined",
        "1",
        "'0'",
        "Date.prototype",
        "Object.create(new Date(0))",
    ] {
        let src = format!(
            "let caught = false; try {{ Date.prototype.toISOString.call({receiver}); }} catch(e) {{ caught = e instanceof TypeError; }} caught"
        );
        assert_eq!(eval(&src), ValueView::Bool(true), "{src}");
    }
}

/// Synthetic fixtures, not light-cljs output: slots, descriptors, coercion
/// ordering, clipping and the intentionally deferred constructor forms.
#[test]
fn date_slots_and_coercion() {
    for src in [
        "Date.name === 'Date' && Date.length === 7 && Date.UTC.name === 'UTC' && Date.UTC.length === 2",
        "Date.prototype.getTime.name === 'getTime' && Date.prototype.getTime.length === 0 && Date.prototype.valueOf.name === 'valueOf' && Date.prototype.valueOf.length === 0 && Date.prototype.setTime.name === 'setTime' && Date.prototype.setTime.length === 1",
        "let d = Object.getOwnPropertyDescriptor(Date, 'prototype'); d.value === Date.prototype && !d.writable && !d.enumerable && !d.configurable",
        "let d = Object.getOwnPropertyDescriptor(Date, 'UTC'); d.value === Date.UTC && d.writable && !d.enumerable && d.configurable",
        "let d = Object.getOwnPropertyDescriptor(Date.prototype, 'getTime'); d.value === Date.prototype.getTime && d.writable && !d.enumerable && d.configurable",
        "let d = new Date(1); d instanceof Date && typeof d === 'object' && Object.getPrototypeOf(d) === Date.prototype && d.constructor === Date && Object.keys(d).length === 0",
        "'toString' in new Date(0) && 'toString' in Date.prototype && 'toJSON' in new Date(0) && 'toJSON' in Date.prototype",
        "Object.prototype.toString.call(new Date(1)) === '[object Date]' && Object.prototype.toString.call(Date.prototype) === '[object Object]'",
        "let d = new Date(7); d.valueOf = function() {throw 1;}; d.toString = function() {throw 2;}; let copy = new Date(d); copy !== d && copy.getTime() === 7",
        "let d = new Date(NaN); Object.defineProperty(d, 'valueOf', {get() {throw 1;}}); Number.isNaN(new Date(d).valueOf())",
        "let d = new Date(7); Object.setPrototypeOf(d, null); Date.prototype.getTime.call(d) === 7 && new Date(d).getTime() === 7",
        "new Date(undefined).getTime() !== new Date(undefined).getTime() && new Date(null).getTime() === 0 && new Date(true).getTime() === 1",
        "new Date(1.9).getTime() === 1 && new Date(-1.9).getTime() === -1 && Object.is(new Date(-0).getTime(), 0) && Object.is(new Date(-0.9).getTime(), 0)",
        "new Date(8.64e15).getTime() === 8.64e15 && new Date(-8.64e15).getTime() === -8.64e15 && Number.isNaN(new Date(-8.64e15 - 1).getTime()) && Number.isNaN(new Date(Infinity).getTime())",
        "let d = new Date(NaN); d.setTime('5.9') === 5 && d.getTime() === 5 && Object.is(d.setTime(-0), 0) && Number.isNaN(d.setTime()) && Number.isNaN(d.getTime())",
        "let d = new Date(1); Object.freeze(d); d.setTime(5) === 5 && d.getTime() === 5",
        "let d = new Date(1); try {d.setTime(Symbol());} catch(e) {} d.getTime() === 1",
        "let d = new Date(1); try {d.setTime({valueOf() {throw 3;}});} catch(e) {} d.getTime() === 1",
        "let calls = ''; let n = new Date({valueOf() {calls += 'v'; return {};}, toString() {calls += 's'; return 3;}}); calls === 'vs' && n.getTime() === 3",
        "let calls = ''; let x = Date.UTC({valueOf() {calls += 'y'; return '1970';}}, {valueOf() {calls += 'm'; return 0;}}); x === 0 && calls === 'ym'",
        "let calls = ''; let x = Date.UTC(NaN, {valueOf() {calls += 'm'; return 0;}}); Number.isNaN(x) && calls === 'm'",
        "let calls = ''; let x = Date.UTC(Infinity, {valueOf() {calls += 'm'; return 0;}}); Number.isNaN(x) && calls === 'm'",
        "let calls = ''; try {Date.UTC(Symbol(), {valueOf() {calls += 'm'; return 0;}});} catch(e) {calls += e instanceof TypeError;} calls === 'true'",
        "let calls = ''; try {Date.UTC({valueOf() {calls += 'y'; throw 3;}}, {valueOf() {calls += 'm'; return 0;}});} catch(e) {calls += e;} calls === 'y3'",
        "Date.UTC(1970, 0, 1, 0, 0, 0, 0, {valueOf() {throw 3;}}) === 0",
        "Date.UTC(99.9) === Date.UTC(1999) && Date.UTC(-0.9) === Date.UTC(1900) && Date.UTC(100) !== Date.UTC(2000)",
        "Date.UTC(1969, 12) === 0 && Date.UTC(1970, -1) === -2678400000 && Date.UTC(1970, 0, 0) === -86400000",
        "Date.UTC(275760, 8, 13) === 8.64e15 && Date.UTC(-271821, 3, 20) === -8.64e15 && Number.isNaN(Date.UTC(275760, 8, 14))",
        "Date.UTC('1970', null, true, false, '0', null, null) === 0 && Number.isNaN(Date.UTC(1970, undefined))",
        "new (Date.bind(null, 9))().getTime() === 9 && Date.UTC.call(null, 1970) === 0 && Date.UTC.apply(null, [1970]) === 0",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
    for receiver in [
        "{}",
        "null",
        "undefined",
        "1",
        "Object.create(Date.prototype)",
        "Date.prototype",
    ] {
        for method in ["getTime", "valueOf", "setTime"] {
            let src = format!(
                "let converted = false; let caught = false; try {{ Date.prototype.{method}.call({receiver}, {{valueOf() {{converted = true; return 1;}}}}); }} catch(e) {{caught = e instanceof TypeError;}} caught && !converted"
            );
            assert_eq!(eval(&src), ValueView::Bool(true), "{src}");
        }
    }
    // ISO construction is supported by clj-quickjs-184; default coercion
    // still awaits Date's Symbol.toPrimitive protocol.
    assert_eq!(
        eval("new Date('2017-09-22').getTime()"),
        ValueView::Number(1506038400000.0)
    );
    // Keep invalid ISO strings on both direct and object-conversion paths;
    // non-ISO numeric strings are reserved for the fallback parser (-185).
    for src in [
        "Number.isNaN(new Date('2000-13').getTime())",
        "Number.isNaN(new Date({valueOf() {return '2000-13';}}).getTime())",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
    assert_eq!(
        failure("'' + new Date(0)").kind(),
        VmErrorKind::UnsupportedOperand
    );
    // Calling Date now uses the host clock; a hostless runtime has none.
    for src in ["Date()", "Date(1)"] {
        assert_eq!(failure(src).kind(), VmErrorKind::RangeError, "{src}");
    }
    for src in [
        "new Date.UTC(1970)",
        "new Date(Symbol())",
        "Date.prototype.setTime(1)",
    ] {
        assert_eq!(failure(src).kind(), VmErrorKind::TypeError, "{src}");
    }
}

/// Synthetic fixtures, not light-cljs output: deferred Date toJSON must
/// not silently turn a Date into an ordinary JSON object. Own hooks still run.
#[test]
fn date_json_serialization() {
    for src in [
        "JSON.stringify(new Date(0))",
        "JSON.stringify(new Date(NaN))",
        "JSON.stringify([new Date(0)])",
        "JSON.stringify({d: new Date(NaN)})",
    ] {
        assert_eq!(
            failure(src).kind(),
            VmErrorKind::UnsupportedOperand,
            "{src}"
        );
    }
    for src in [
        "let d = new Date(0); d.toJSON = function() {return 'custom';}; JSON.stringify(d) === '\"custom\"'",
        "let d = new Date(NaN); let key; Object.defineProperty(d, 'toJSON', {get() {return function(k) {key = k; return 42;};}}); JSON.stringify({x: d}) === '{\"x\":42}' && key === 'x'",
        "let d = new Date(0); d.toJSON = undefined; JSON.stringify(d) === '{}'",
        "let d = new Date(0); d.toJSON = 7; JSON.stringify(d) === '{\"toJSON\":7}'",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
}

/// Synthetic fixtures, not light-cljs output: bound leaf numeric string scans
/// after observable coercion and before visiting later arguments or mutation.
#[test]
fn date_numeric_conversion_limit() {
    for src in [
        "Date.UTC('1970'.padStart(4096, ' ')) === 0",
        "let d = new Date(0); d.setTime('5'.padStart(4096, ' ')) === 5",
        "let log = ''; let caught = false; try {Date.UTC({valueOf() {log += 'y'; return '1970'.padStart(4097, ' ');}}, {valueOf() {log += 'm'; return 0;}});} catch(e) {caught = e instanceof RangeError;} caught && log === 'y'",
        "let d = new Date(1); let calls = 0; let caught = false; try {d.setTime({valueOf() {calls++; return '5'.padStart(4097, ' ');}});} catch(e) {caught = e instanceof RangeError;} caught && calls === 1 && d.getTime() === 1",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
}

/// Synthetic fixture, not light-cljs output. Exercise both pending arguments
/// and an otherwise unreachable setTime receiver through waits, GC and migration.
#[test]
fn date_coercion_waits_collects_and_migrates() {
    let rt = Runtime::new();
    let src = r"
        let log = '';
        let d = new Date({get valueOf() {
            log += 'g'; __rt.need(1, 1);
            return function() {log += 'v'; __rt.need(2, 1); return 12;};
        }});
        d.extra = {n: 42};
        let set = new Date(1).setTime({valueOf() {log += 's'; __rt.need(3, 1); return 5;}});
        let utc = Date.UTC({valueOf() {log += 'y'; __rt.need(4, 1); return 1970;}},
                           {valueOf() {log += 'm'; __rt.need(5, 1); return 0;}});
        let invalid = new Date(NaN);
        invalid.setTime({valueOf() {log += 'i'; __rt.need(6, 1); return 9;}});
        d.getTime() === 12 && d.extra.n === 42 && set === 5 && utc === 0
            && invalid.getTime() === 9 && log === 'gvsymi'
    ";
    let mut task = rt.spawn(&exe(src), &[]).unwrap();
    let mut waited = 0;
    let mut complete = false;
    for _ in 0..2000 {
        let (next, outcome) = std::thread::spawn(move || {
            let outcome = task
                .run_with(Budget::instructions(1), &mut NoEffects)
                .unwrap();
            (task, outcome)
        })
        .join()
        .unwrap();
        task = next;
        match outcome {
            RunOutcome::Yielded => {
                rt.collect().unwrap();
            }
            RunOutcome::Waiting(_) => {
                waited += 1;
                assert!(waited <= 6);
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
            RunOutcome::Complete(value) => {
                assert_eq!(value.view(), ValueView::Bool(true));
                complete = true;
                break;
            }
            other => panic!("{other:?}"),
        }
    }
    assert!(complete);
    assert_eq!(waited, 6);
}

/// Synthetic fixtures, not light-cljs output: ordinary Date slots never permit
/// publication or foreign-task reads, even after freezing or detaching prototypes.
#[test]
fn date_objects_are_task_owned() {
    use clj_quickjs::PublishError;
    let rt = Runtime::new();
    for source in ["new Date(7)", "Object.freeze(new Date(7))"] {
        let owner = rt.spawn(&exe(source), &[]).unwrap();
        let RunOutcome::Complete(date) = owner.run().unwrap() else {
            panic!("expected Date");
        };
        assert!(matches!(
            rt.publish(&date),
            Err(PublishError::NotShareable { .. })
        ));
        for source in [
            "function f(d) {return Date.prototype.getTime.call(d);}",
            "function f(d) {return Date.prototype.setTime.call(d, 1);}",
            "function f(d) {return Date.prototype.toISOString.call(d);}",
            "function f(d) {return Date.prototype.getUTCFullYear.call(d);}",
            "function f(d) {return Date.prototype.setUTCHours.call(d, 1);}",
            "function f(d) {return new Date(d);}",
        ] {
            let reader = rt
                .spawn(&compile_function(source).unwrap(), &[&date])
                .unwrap();
            assert!(
                matches!(reader.run().unwrap(), RunOutcome::Failed(e)
                if matches!(e.kind(), VmErrorKind::ForeignObject { .. })),
                "{source}"
            );
        }
    }
}

/// Synthetic fixtures, not light-cljs output. The first two rows adapt
/// QuickJS tests/test_builtin.js::test_array, revision 7d1c13c5d8f8;
/// its full copyright and MIT permission notice is retained on array_push.
#[test]
fn array_constructor_and_is_array() {
    for src in [
        "let a = new Array(10); a.length === 10",
        "let a = new Array(1, 2); a.length === 2 && a[0] === 1 && a[1] === 2",
        "new Array(10)[0] === undefined && Object.keys(new Array(3)).length === 0",
        "Array(3).length === 3 && Array().length === 0 && new Array().length === 0",
        "let a = new Array('3'); a.length === 1 && a[0] === '3'",
        "Array(1, 2)[1] === 2 && new Array(1, 2, 3)[2] === 3",
        "Array.isArray([]) && Array.isArray(new Array()) && Array.isArray(Array.prototype)",
        "!Array.isArray({length: 0}) && !Array.isArray(__rt.vector(1)) && !Array.isArray(Object.create(Array.prototype))",
        "!Array.isArray() && !Array.isArray(null) && !Array.isArray(undefined) && !Array.isArray(3) && !Array.isArray('') && !Array.isArray(true) && !Array.isArray(Array)",
        "[].constructor === Array && Array.prototype.constructor === Array && Array.name === 'Array' && Array.length === 1",
        "Object.getPrototypeOf(new Array()) === Array.prototype && new Array() instanceof Array",
        "let d = Object.getOwnPropertyDescriptor(Array, 'prototype'); d.value === Array.prototype && !d.writable && !d.enumerable && !d.configurable",
        "let d = Object.getOwnPropertyDescriptor(Array.prototype, 'constructor'); d.value === Array && d.writable && !d.enumerable && d.configurable",
        "let d = Object.getOwnPropertyDescriptor(Array, 'isArray'); d.value === Array.isArray && d.writable && !d.enumerable && d.configurable && d.value.name === 'isArray' && d.value.length === 1",
        "let a = Array(undefined); a.length === 1 && 0 in a && a[0] === undefined",
        "Array(null)[0] === null && Array(true)[0] === true && Array(-0).length === 0 && 1 / Array(-0).length === Infinity",
        "let a = Array(4294967295); a.length === 4294967295 && Object.keys(a).length === 0 && !(4294967294 in a)",
        "let a = new Array(4294967295); a[4294967294] = 7; a.length === 4294967295 && a[4294967294] === 7 && Object.keys(a).length === 1",
        "let a = Array(3); a.push(7) === 4 && a[3] === 7 && !(0 in a)",
        "let o = {get valueOf() { throw 42; }, get toString() { throw 43; }}; Array(o)[0] === o && new Array(o)[0] === o && !Array.isArray(o)",
        "let o = new Number(3); let a = new Array(o); a.length === 1 && a[0] === o",
        "Array.call(null, 2).length === 2 && Array.apply(null, [1, 2])[1] === 2 && new (Array.bind(null, 1, 2))()[0] === 1",
        "let a = Array(1); let b = Array(1); a[0] = 7; a !== b && !(0 in b)",
        "Object.prototype.toString.call(Array.prototype) === '[object Array]'",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
    for length in ["-1", "1.5", "4294967296", "NaN", "Infinity", "-Infinity"] {
        for constructor in ["Array", "new Array"] {
            let src = format!("{constructor}({length})");
            assert_eq!(failure(&src).kind(), VmErrorKind::RangeError, "{src}");
            let caught = format!(
                "let caught = false; try {{ {src}; }} catch (e) {{ caught = true; }} caught"
            );
            assert_eq!(eval(&caught), ValueView::Bool(true), "{src}");
        }
    }
    assert_eq!(
        failure("new Array.isArray([])").kind(),
        VmErrorKind::TypeError
    );
}

/// Synthetic fixtures, not light-cljs output: constructor results follow
/// the same task ownership and publication rules as array literals.
#[test]
fn constructed_arrays_are_task_owned() {
    use clj_quickjs::PublishError;
    let rt = Runtime::new();
    for source in ["Array(3)", "new Array({n: 7}, 2)"] {
        let owner = rt.spawn(&exe(source), &[]).unwrap();
        let RunOutcome::Complete(array) = owner.run().unwrap() else {
            panic!("expected array from {source}");
        };
        assert!(matches!(
            rt.publish(&array),
            Err(PublishError::NotShareable { .. })
        ));
        let code = compile_function("function f(a) { return a.length; }").unwrap();
        let foreign = rt.spawn(&code, &[&array]).unwrap();
        assert!(matches!(foreign.run().unwrap(), RunOutcome::Failed(e)
            if matches!(e.kind(), VmErrorKind::ForeignObject { .. })));
        let code = compile_function("function f(a) { return Array.isArray(a); }").unwrap();
        let reader = rt.spawn(&code, &[&array]).unwrap();
        let RunOutcome::Complete(result) = reader.run().unwrap() else {
            panic!("array brand inspection must not read foreign properties");
        };
        assert_eq!(result.view(), ValueView::Bool(true));
    }
}

/// Synthetic fixtures, not light-cljs output. SameValue comparisons cover
/// NaN and distinguish signed zeros for both exponentiation entry points.
#[test]
fn math_pow() {
    for (base, exponent, check) in [
        ("2", "32", "x - 1 === 4294967295"),
        ("2", "-1", "x === 0.5"),
        ("NaN", "0", "x === 1"),
        ("1", "Infinity", "Number.isNaN(x)"),
        ("-8", "1/3", "Number.isNaN(x)"),
        ("-0", "-1", "x === -Infinity"),
        ("-0", "3", "1 / x === -Infinity"),
        ("-1", "-Infinity", "Number.isNaN(x)"),
        ("1", "NaN", "Number.isNaN(x)"),
        ("NaN", "-0", "x === 1"),
        ("-0", "2", "1 / x === Infinity"),
        ("-Infinity", "-3", "1 / x === -Infinity"),
    ] {
        let src = format!(
            "let x = Math.pow({base}, {exponent}); ({check}) && Object.is(x, ({base}) ** ({exponent}))"
        );
        assert_eq!(eval(&src), ValueView::Bool(true), "{src}");
    }
    for src in [
        "let d = Object.getOwnPropertyDescriptor(Math, 'pow'); d.value === Math.pow && d.writable && !d.enumerable && d.configurable && Math.pow.length === 2 && Math.pow.name === 'pow'",
        "Number.isNaN(Math.pow()) && Number.isNaN(Math.pow(2)) && Number.isNaN(Math.pow(undefined, 2))",
        "Math.pow('2', '3') === 8 && Math.pow(null, false) === 1",
        "Math.pow.call(null, 2, 3) === 8 && Math.pow.apply(null, [2, 3]) === 8 && Math.pow.bind(null, 2)(3) === 8",
        "let log = ''; function v(n) { return {valueOf() { log += n; return n; }}; } Math.pow(v(2), v(3)) === 8 && log === '23'",
        "let log = ''; let a = {valueOf() { log += 'a'; return NaN; }}; let b = {valueOf() { log += 'b'; return 0; }}; Math.pow(a, b) === 1 && log === 'ab'",
        "let log = ''; let a = {valueOf() { throw 42; }}; let b = {valueOf() { log += 'b'; return 0; }}; let caught = false; try { Math.pow(a, b); } catch (e) { caught = e === 42; } caught && log === ''",
        "let caught = false; try { Math.pow(NaN, {valueOf() { throw 42; }}); } catch (e) { caught = e === 42; } caught",
        "let o = {valueOf() { throw 42; }}; Math.pow(2, 3, o) === 8",
        "let o = {valueOf() { return {}; }, toString() { return '3'; }}; Math.pow(new Number(2), o) === 8",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
    assert_eq!(failure("new Math.pow(2, 3)").kind(), VmErrorKind::TypeError);
}

/// Synthetic fixtures, not light-cljs output.
#[test]
fn math_sqrt_and_log2() {
    for src in [
        "Math.sqrt(4) === 2 && Math.sqrt(2) === 1.4142135623730951",
        "1 / Math.sqrt(-0) === -Infinity && 1 / Math.sqrt(0) === Infinity",
        "Number.isNaN(Math.sqrt(-1)) && Number.isNaN(Math.sqrt(-Infinity)) && Number.isNaN(Math.sqrt(NaN))",
        "Math.sqrt(Infinity) === Infinity && Math.sqrt(Number.MIN_VALUE) === 2 ** -537",
        "Math.log2(8) === 3 && Math.log2(2 ** -1074) === -1074",
        "Math.log2(10) === 3.321928094887362 && Math.ceil(2000 * Math.log2(10)) === 6644",
        "Math.log2(0) === -Infinity && Math.log2(-0) === -Infinity",
        "Number.isNaN(Math.log2(-1)) && Number.isNaN(Math.log2(-Infinity)) && Number.isNaN(Math.log2(NaN))",
        "Math.log2(Infinity) === Infinity && 1 / Math.log2(1) === Infinity",
        "Number.isNaN(Math.sqrt()) && Number.isNaN(Math.log2()) && Number.isNaN(Math.sqrt(undefined))",
        "Math.sqrt('9') === 3 && Math.sqrt(null) === 0 && Math.log2(true) === 0 && Math.log2(null) === -Infinity",
        "Math.sqrt.call(null, '16') === 4 && Math.log2.apply(null, [32]) === 5 && Math.sqrt.bind(null, 25)() === 5",
        "let log = ''; let o = {valueOf() {log += 'v'; return {};}, toString() {log += 's'; return '16';}}; Math.sqrt(o) === 4 && Math.log2(o) === 4 && log === 'vsvs'",
        "let o = {valueOf() {throw 42;}}; Math.sqrt(4, o) === 2 && Math.log2(8, o) === 3",
        "let ok = true; for (let e = -1074; e <= 1023; e++) { if (Math.log2(2 ** e) !== e) {ok = false;} } ok",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
    for name in ["sqrt", "log2"] {
        let src = format!(
            "let f = Math.{name}; let d = Object.getOwnPropertyDescriptor(Math, '{name}'); d.value === f && d.writable && !d.enumerable && d.configurable && f.length === 1 && f.name === '{name}' && Object.getPrototypeOf(f) === Function.prototype"
        );
        assert_eq!(eval(&src), ValueView::Bool(true), "{src}");
        assert_eq!(
            failure(&format!("new Math.{name}()")).kind(),
            VmErrorKind::TypeError
        );
        assert_eq!(
            failure(&format!("Math.{name}(Object.create(null))")).kind(),
            VmErrorKind::TypeError
        );
        let src = format!(
            "let caught = false; try {{ Math.{name}({{valueOf() {{throw 42;}}}}); }} catch(e) {{caught = e === 42;}} caught"
        );
        assert_eq!(eval(&src), ValueView::Bool(true), "{src}");
    }
}

/// Synthetic fixtures, not light-cljs output (clj-quickjs-58).
#[test]
fn tagged_templates() {
    for src in [
        r#"let b = 123; String.raw `abc${b}d` === 'abc123d'"#,
        r#"String.raw`a\nb` === 'a\\nb' && String.raw`a\nb`.length === 4"#,
        r#"function tag(s) { return s; } function site() { return tag`x${1}y`; } let a = site(), b = site(); a === b && a !== tag`x${1}y` && a !== tag`x${1}y` && a[0] === 'x' && a[1] === 'y' && a.length === 2 && Object.isFrozen(a) && Object.isFrozen(a.raw) && Array.isArray(a) && Array.isArray(a.raw) && Object.getPrototypeOf(a) === Array.prototype"#,
        r#"function tag(s) { let d = Object.getOwnPropertyDescriptor(s, 'raw'); let e = Object.getOwnPropertyDescriptor(s, '0'); return !d.writable && !d.enumerable && !d.configurable && !e.writable && e.enumerable && !e.configurable && Object.freeze(s) === s; } tag`x`"#,
        r#"function tag(s) { return s[0] === undefined && s.raw[0] === '\\unicode' && s.raw[0].length === 8; } tag`\unicode`"#,
        r#"let log = ''; function tag(s, x) { log += s[0]; return {get value() { log += 'v'; return x; }}; } tag`a${log += 's'}`.value === 's' && log === 'sav'"#,
        r#"let log = ''; function tag(s, x) { log += s[0]; return function(t, y) { log += t[0]; return x + y; }; } tag`a${log += '1'}``b${log += '2'}` === '11a2' && log === '1a2b'"#,
        r#"let o = {tag(s, x) { return this === o && x === 3; }}; o.tag`x${3}` && o['tag']`y${3}`"#,
        r#"let n = 0; function tag(s, x) { return x; } let x = {toString() { n++; return 'x'; }}; tag`${x}` === x && n === 0"#,
        r#"function tag(s) { return s; } let a = tag`x`; let caught = 0; try { a[0] = 'y'; } catch (e) { caught++; } try { a.raw = []; } catch (e) { caught++; } try { a.raw[0] = 'z'; } catch (e) { caught++; } caught === 3 && a[0] === 'x' && a.raw[0] === 'x'"#,
        r#"String.raw({raw: ['a', 'b', 'c']}, 1) === 'a1bc' && String.raw({raw: []}, 1) === '' && String.raw({raw: 'xy'}, 2, 3) === 'x2y'"#,
        r#"let log = ''; let raw = {get length() { log += 'l'; return {valueOf() { log += 'n'; return 2; }}; }, get 0() { log += 'a'; return {toString() { log += 's'; return 'A'; }}; }, get 1() { log += 'b'; return 'B'; }}; let site = {get raw() { log += 'r'; return raw; }}; String.raw(site, {toString() { log += 'x'; return 'X'; }}) === 'AXB' && log === 'rlnasxb'"#,
        r#"String.raw.name === 'raw' && String.raw.length === 1 && !Object.getOwnPropertyDescriptor(String, 'raw').enumerable"#,
        r#"function tag(s) { return s; } function make() { return () => tag`x`; } let a = make(), b = make(); a() === b()"#,
        r#"let log = ''; let o = {get tag() { log += 'g'; return function(s, x) { log += 't'; return this === o; }; }}; o.tag`${log += 's'}` && log === 'gst'"#,
        r#"function tag(s) { return function C(x) { this.x = x; }; } (new tag`x`(42)).x === 42"#,
        r#"let log = ''; let bad = {toString() { log += 'x'; throw 42; }}; let later = {get 1() { log += 'b'; return 'B'; }, 0: 'A', length: 2}; let caught = false; try { String.raw({raw: later}, bad); } catch (e) { caught = e === 42; } caught && log === 'x'"#,
        r#"String.raw({raw: {length: 2.9}}, undefined) === 'undefinedundefinedundefined' && String.raw({raw: {length: -1}}, {toString() { throw 42; }}) === ''"#,
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
    for src in [
        "String.raw()",
        "String.raw({})",
        "String.raw({raw: null})",
        "new String.raw({raw: []})",
    ] {
        assert_eq!(failure(src).kind(), VmErrorKind::TypeError, "{src}");
    }
}

/// Synthetic fixture: completed raw parts use the shared bounded copier.
#[test]
fn tagged_templates_raw_copy_is_budgeted() {
    let text = "a".repeat(8191);
    let rt = Runtime::new();
    let code = exe(&format!("String.raw({{raw: ['{text}', 'z']}}, 1)"));
    let task = rt.spawn(&code, &[]).unwrap();
    let mut yields = 0;
    let mut complete = false;
    for _ in 0..256 {
        match task
            .run_with(Budget::instructions(1), &mut NoEffects)
            .unwrap()
        {
            RunOutcome::Yielded => {
                yields += 1;
                rt.collect().unwrap();
            }
            RunOutcome::Complete(value) => {
                assert_eq!(value.string().unwrap().to_string(), format!("{text}1z"));
                complete = true;
                break;
            }
            other => panic!("{other:?}"),
        }
    }
    assert!(complete && yields > 32);
}

/// Synthetic fixture: caches are realm roots; their immutable arrays
/// publish across tasks, while shared code does not carry heap/cache state.
#[test]
fn tagged_templates_publish_and_cache_per_realm() {
    let rt = Runtime::new();
    let code = exe("function tag(s) { return s; } tag`x${1}y`");
    let code = Executable::load(code.to_spec()).unwrap();
    let producer = rt.spawn(&code, &[]).unwrap();
    let RunOutcome::Complete(first) = producer.run().unwrap() else {
        panic!("producer");
    };
    rt.publish(&first).unwrap();
    drop(producer);
    rt.collect().unwrap();
    let probe = compile_function("function probe(s) { return Object.isFrozen(s) && Object.isFrozen(s.raw) && s[0] === 'x' && s.raw[1] === 'y' && Object.freeze(s) === s; }").unwrap();
    let RunOutcome::Complete(result) = rt.spawn(&probe, &[&first]).unwrap().run().unwrap() else {
        panic!("probe");
    };
    assert_eq!(result.view(), ValueView::Bool(true));
    let RunOutcome::Complete(second) = rt.spawn(&code, &[]).unwrap().run().unwrap() else {
        panic!("second");
    };
    rt.scope(|s| assert!(s.same_object(s.local(&first).unwrap(), s.local(&second).unwrap())));
    let other = Runtime::new();
    let RunOutcome::Complete(foreign) = other.spawn(&code, &[]).unwrap().run().unwrap() else {
        panic!("other realm");
    };
    assert!(!first.same_object(&foreign));
    // A closure that captures the array can be published with its code.
    let code = exe("const s = (function(s) { return s; })`x`; () => s.raw[0]");
    let RunOutcome::Complete(closure) = rt.spawn(&code, &[]).unwrap().run().unwrap() else {
        panic!("closure");
    };
    rt.publish(&closure).unwrap();
    let call = compile_function("function call(f) { return f() === 'x'; }").unwrap();
    let RunOutcome::Complete(result) = rt.spawn(&call, &[&closure]).unwrap().run().unwrap() else {
        panic!("call");
    };
    assert_eq!(result.view(), ValueView::Bool(true));
}

/// Synthetic fixture: published code preserves the site's identity across tasks.
#[test]
fn tagged_templates_published_site_identity() {
    let rt = Runtime::new();
    let code = exe("const site = () => (s => s)`x`; const first = site(); () => site() === first");
    let RunOutcome::Complete(closure) = rt.spawn(&code, &[]).unwrap().run().unwrap() else {
        panic!("producer");
    };
    rt.publish(&closure).unwrap();
    rt.collect().unwrap();
    let call = compile_function("function call(f) { return f(); }").unwrap();
    let RunOutcome::Complete(result) = rt.spawn(&call, &[&closure]).unwrap().run().unwrap() else {
        panic!("consumer");
    };
    assert_eq!(result.view(), ValueView::Bool(true));
}

/// Synthetic fixture: bounded start gates coordinate two first evaluations.
#[test]
fn tagged_templates_concurrent_first_install() {
    use std::sync::mpsc;
    use std::time::Duration;
    let timeout = Duration::from_secs(30);
    let rt = Runtime::new();
    let code = exe("(s => s)`shared`");
    let mut gates = Vec::new();
    let mut workers = Vec::new();
    let (ready_tx, ready_rx) = mpsc::channel();
    let (result_tx, result_rx) = mpsc::channel();
    for _ in 0..2 {
        let task = rt.spawn(&code, &[]).unwrap();
        let (gate_tx, gate_rx) = mpsc::channel();
        let ready = ready_tx.clone();
        let result = result_tx.clone();
        gates.push(gate_tx);
        workers.push(std::thread::spawn(move || {
            ready.send(()).unwrap();
            gate_rx.recv_timeout(timeout).unwrap();
            let RunOutcome::Complete(value) = task.run().unwrap() else {
                panic!("worker");
            };
            result.send(value).unwrap();
        }));
    }
    for _ in 0..2 {
        ready_rx.recv_timeout(timeout).unwrap();
    }
    for gate in gates {
        gate.send(()).unwrap();
    }
    let first = result_rx.recv_timeout(timeout).unwrap();
    let second = result_rx.recv_timeout(timeout).unwrap();
    for worker in workers {
        worker.join().unwrap();
    }
    rt.collect().unwrap();
    assert!(first.same_object(&second));
}

/// Early VM tests consume arrays created by source: literal append opcodes
/// must not bypass descriptors and mutate published template arrays.
#[test]
fn tagged_templates_reject_literal_append() {
    let rt = Runtime::new();
    for source in ["(s => s)`x`", "(s => s.raw)`x`"] {
        let RunOutcome::Complete(array) = rt.spawn(&exe(source), &[]).unwrap().run().unwrap()
        else {
            panic!("producer");
        };
        rt.publish(&array).unwrap();
        for op in [Opcode::AppendElement, Opcode::AppendHole] {
            let mut asm = clj_quickjs::Assembler::new();
            asm.get_local(0);
            if op == Opcode::AppendElement {
                asm.op(Opcode::NewArray);
            }
            asm.op(op).op(Opcode::Return);
            let code = asm.finish("append", 1, 1).unwrap();
            let RunOutcome::Failed(error) = rt.spawn(&code, &[&array]).unwrap().run().unwrap()
            else {
                panic!("append accepted {op:?}");
            };
            assert_eq!(error.kind(), VmErrorKind::TypeError);
            rt.collect().unwrap();
            rt.publish(&array).unwrap();
            let check = compile_function("function check(a) { return a.length === 1 && a[0] === 'x' && !('1' in a) && Object.isFrozen(a); }").unwrap();
            let RunOutcome::Complete(result) = rt.spawn(&check, &[&array]).unwrap().run().unwrap()
            else {
                panic!("probe");
            };
            assert_eq!(result.view(), ValueView::Bool(true));
        }
    }
}

/// Synthetic fixtures: descriptor no-ops and strict/sloppy deletes on arrays
/// shared from a producer task obey frozen descriptors, not ownership errors.
#[test]
fn tagged_templates_descriptor_operations() {
    let rt = Runtime::new();
    for source in ["(s => s)`x`", "(s => s.raw)`x`"] {
        let RunOutcome::Complete(array) = rt.spawn(&exe(source), &[]).unwrap().run().unwrap()
        else {
            panic!("producer");
        };
        rt.publish(&array).unwrap();
        let noop = compile_function("function noop(a) { return Object.defineProperty(a, '0', {value: 'x', writable: false, enumerable: true, configurable: false}) === a && Object.defineProperty(a, 'length', {value: 1}) === a && delete a.absent; }").unwrap();
        let RunOutcome::Complete(result) = rt.spawn(&noop, &[&array]).unwrap().run().unwrap()
        else {
            panic!("descriptor no-op");
        };
        assert_eq!(result.view(), ValueView::Bool(true));
        for expression in [
            "Object.defineProperty(a, '0', {value: 'changed'})",
            "Object.defineProperty(a, '0', {writable: true})",
            "Object.defineProperty(a, 'extra', {value: 1})",
            "Object.defineProperty(a, 'length', {value: 2})",
            "delete a[0]",
            "delete a.length",
        ] {
            let code = compile_function(&format!("function change(a) {{ return {expression}; }}"))
                .unwrap();
            let RunOutcome::Failed(error) = rt.spawn(&code, &[&array]).unwrap().run().unwrap()
            else {
                panic!("mutation accepted: {expression}");
            };
            assert_eq!(error.kind(), VmErrorKind::TypeError, "{expression}");
        }
        let sloppy = compile_function_with("function check(a) { return delete a[0] === false && delete a.length === false && delete a.absent === true && a.length === 1 && a[0] === 'x' && !('extra' in a) && Object.isFrozen(a); }", ScriptOptions::SLOPPY).unwrap();
        rt.collect().unwrap();
        let RunOutcome::Complete(result) = rt.spawn(&sloppy, &[&array]).unwrap().run().unwrap()
        else {
            panic!("sloppy delete");
        };
        assert_eq!(result.view(), ValueView::Bool(true));
        rt.publish(&array).unwrap();
    }
}

/// Synthetic fixture: each getter/conversion runs once through waits,
/// single-instruction yields, collection and migration to fresh threads.
#[test]
fn tagged_templates_wait_yield_collect_and_migrate() {
    let rt = Runtime::new();
    let code = exe(r#"
        function tag(s) { return s; }
        function site() { return tag`cached`; }
        site();
        let log = '';
        let result = String.raw({get raw() {
            log += 'r'; __rt.need(1, 1);
            return {get length() { log += 'l'; return {valueOf() {
                log += 'n'; __rt.need(2, 1); return 2;
            }}; }, get 0() { log += 'a'; return {toString() {
                log += 's'; __rt.need(3, 1); return 'A';
            }}; }, get 1() { log += 'b'; __rt.need(5, 1); return 'B'; }};
        }}, {toString() { log += 'x'; __rt.need(4, 1); return 'X'; }});
        result === 'AXB' && log === 'rlnasxb' && site()[0] === 'cached'
    "#);
    let mut task = rt.spawn(&code, &[]).unwrap();
    let mut waited = 0;
    let mut complete = false;
    for _ in 0..1000 {
        let (next, outcome) = std::thread::spawn(move || {
            let outcome = task
                .run_with(Budget::instructions(1), &mut NoEffects)
                .unwrap();
            (task, outcome)
        })
        .join()
        .unwrap();
        task = next;
        match outcome {
            RunOutcome::Yielded => {
                rt.collect().unwrap();
            }
            RunOutcome::Waiting(_) => {
                waited += 1;
                assert!(waited <= 5);
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
            RunOutcome::Complete(value) => {
                assert_eq!(value.view(), ValueView::Bool(true));
                complete = true;
                break;
            }
            other => panic!("{other:?}"),
        }
    }
    assert!(complete);
    assert_eq!(waited, 5);
}

/// Synthetic fixtures, not light-cljs output. The first rows adapt test_math
/// in QuickJS tests/test_builtin.js (7d1c13c5d8f8); the full QuickJS copyright
/// and MIT permission notice is retained on string_code_unit_access below.
#[test]
fn math_basic_functions() {
    for src in [
        "Math.floor(1.4) === 1 && Math.ceil(1.4) === 2",
        "Math.imul(0x12345678, 123) === -1088058456",
        "Math.imul(0xB505, 0xB504) === 2147441940 && Math.imul(0xB505, 0xB505) === -2147479015",
        "Math.imul((-2)**31, (-2)**31) === 0 && Math.imul(2**31-1, 2**31-1) === 1",
        "Math.fround(0.1) === 0.10000000149011612",
        "Math.hypot() === 0 && Math.hypot(-2) === 2 && Math.hypot(3, 4) === 5",
        "Math.abs(Math.hypot(3, 4, 5) - 7.0710678118654755) <= 1e-15",
        // User-approved spec correction (2026-10-24): floor(-0.5) is -1.
        "Math.floor(-0.5) === -1 && 1 / Math.floor(-0) === -Infinity && 1 / Math.ceil(-0.5) === -Infinity",
        "Math.abs(-2) === 2 && 1 / Math.abs(-0) === Infinity && Math.floor('2.5') === 2",
        "Math.imul(-1, 8) === -8 && Math.imul(3) === 0 && Math.imul(4294967297.9, '3') === 3",
        "Number.isNaN(Math.fround(NaN)) && Math.fround(2**128) === Infinity && Math.fround(1.5) === 1.5",
        "Math.fround(1 + 2**-24) === 1 && Math.fround(1 + 3 * 2**-24) === 1 + 2**-22",
        "1 / Math.fround(-1e-100) === -Infinity && 1 / Math.fround(-0) === -Infinity",
        "Math.hypot(NaN, Infinity) === Infinity && Math.hypot(Infinity, NaN, -Infinity) === Infinity",
        "Number.isNaN(Math.hypot(NaN, 1)) && 1 / Math.hypot(-0) === Infinity",
        "Math.hypot(1e200, 1e200) < Infinity && Math.hypot(1e-200, 1e-200) > 0",
        "Math.hypot(3e200, 4e200) / 1e200 === 5 && Math.hypot(3e-200, 4e-200) / 1e-200 === 5",
        "Math.hypot(Number.MIN_VALUE) === Number.MIN_VALUE && Math.hypot(Number.MAX_VALUE) === Number.MAX_VALUE",
        "let a = [1]; for (let i = 0; i < 128; i++) { a.push(2**-27); } Math.hypot.apply(null, a) === 1 + 2**-48",
        "Number.isNaN(Math.floor()) && Number.isNaN(Math.ceil()) && Number.isNaN(Math.abs()) && Number.isNaN(Math.fround()) && Math.imul() === 0",
        "Math.floor(Infinity) === Infinity && Math.ceil(-Infinity) === -Infinity && Math.abs(-Infinity) === Infinity",
        "typeof Math === 'object' && Object.getPrototypeOf(Math) === Object.prototype && Object.keys(Math).length === 0",
        "Math.floor.call(null, '2.9') === 2 && Math.imul.apply(null, [-1, 8]) === -8 && Math.hypot.bind(null, 3)(4) === 5",
        "let Math = 7; Math === 7",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
    for (name, length) in [
        ("floor", 1),
        ("ceil", 1),
        ("abs", 1),
        ("imul", 2),
        ("fround", 1),
        ("hypot", 2),
    ] {
        let src = format!(
            "let f = Math['{name}']; let d = Object.getOwnPropertyDescriptor(Math, '{name}'); d.value === f && d.writable && !d.enumerable && d.configurable && f.length === {length} && f.name === '{name}'"
        );
        assert_eq!(eval(&src), ValueView::Bool(true), "{name}");
        assert_eq!(
            failure(&format!("new Math.{name}()")).kind(),
            VmErrorKind::TypeError
        );
    }
    for src in ["Math()", "new Math()"] {
        let caught =
            format!("let caught = false; try {{ {src}; }} catch (e) {{ caught = true; }} caught");
        assert_eq!(eval(&caught), ValueView::Bool(true), "{src}");
        assert_eq!(failure(src).kind(), VmErrorKind::TypeError);
    }
}

/// Synthetic fixtures, not light-cljs output: ToNumber is ordered and does
/// not stop at Infinity or NaN; unused operands are never coerced.
#[test]
fn math_coercion() {
    for src in [
        "let log = ''; function v(n) { return {valueOf() { log += n; return n; }}; } Math.imul(v(2), v(3)) === 6 && log === '23'",
        "let log = ''; let o = {valueOf() { log += 'v'; return {}; }, toString() { log += 's'; return '-2.5'; }}; Math.floor(o) === -3 && Math.ceil(o) === -2 && Math.abs(o) === 2.5 && Math.fround(o) === -2.5 && log === 'vsvsvsvs'",
        "let log = ''; let o = {valueOf() { log += 'v'; return 4; }}; Math.hypot(NaN, Infinity, o) === Infinity && log === 'v'",
        "let caught = false; try { Math.hypot(Infinity, {valueOf() { throw 42; }}); } catch (e) { caught = e === 42; } caught",
        "let log = ''; let o = {valueOf() { log += 'v'; return NaN; }}; let p = {valueOf() { log += 'p'; return Infinity; }}; Math.hypot(o, p) === Infinity && log === 'vp'",
        "let caught = false; try { Math.imul({valueOf() { throw 42; }}, {valueOf() { throw 43; }}); } catch (e) { caught = e === 42; } caught",
        "let o = {valueOf() { throw 42; }}; Math.floor(2, o) === 2 && Math.imul(2, 3, o) === 6",
        "Math.hypot({valueOf: Math.abs.bind(null, -3)}, new Number(4)) === 5",
        "__rt.reduce(Math.imul, 2, __rt.vector(3, 4)) === 24 && __rt.reduce(Math.hypot, 3, __rt.vector(4)) === 5",
        "let log = ''; let o = {get valueOf() { log += 'g'; return function() { log += 'v'; return this.n; }; }, n: -2.5}; Math.floor(o) === -3 && log === 'gv'",
        "let log = ''; let o = {get valueOf() { log += 'v'; return 0; }, get toString() { log += 's'; return () => '4'; }}; Math.hypot(3, o) === 5 && log === 'vs'",
        "let caught = false; try { Math.hypot(Infinity, {get valueOf() { throw 42; }}); } catch (e) { caught = e === 42; } caught",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
    assert_eq!(
        failure("Math.hypot(Infinity, Object.create(null))").kind(),
        VmErrorKind::TypeError
    );
}

/// Synthetic fixture, not light-cljs output. The wait excludes array setup
/// from the measured budget: either unbudgeted numeric pass loses half the
/// required yields even though the final numerical result would still pass.
#[test]
fn math_hypot_primitive_work_is_budgeted() {
    const COUNT: usize = 4096;
    const SLICE: usize = 64;
    let rt = Runtime::new();
    let script = exe(&format!(
        "let a = []; for (let i = 0; i < {COUNT}; i++) {{ a.push(1); }} \
         __rt.need(152, 1); Math.hypot.apply(null, a)"
    ));
    let mut task = rt.spawn(&script, &[]).unwrap();
    assert!(matches!(task.run().unwrap(), RunOutcome::Waiting(_)));
    let request = task.take_producer_requests().pop().unwrap();
    let ready = rt.root(Primitive::Number(0.0));
    task = rt
        .complete(request.key(), request.producer(), Completion::Ready(&ready))
        .unwrap()
        .into_runnable()
        .pop()
        .unwrap();
    let mut yields = 0;
    for _ in 0..4 * COUNT / SLICE {
        match task
            .run_with(Budget::instructions(SLICE as u64), &mut NoEffects)
            .unwrap()
        {
            RunOutcome::Yielded => {
                yields += 1;
                rt.collect().unwrap();
            }
            RunOutcome::Complete(value) => {
                assert_eq!(value.view(), ValueView::Number(64.0));
                assert!(
                    yields >= 2 * COUNT / SLICE,
                    "both ToNumber and scaled summation must consume budget: {yields} yields"
                );
                return;
            }
            other => panic!("primitive hypot: {other:?}"),
        }
    }
    panic!("primitive hypot exceeded its instruction bound");
}

/// Synthetic fixtures, not light-cljs output. Conversion receivers and
/// unconverted arguments stay rooted while callbacks wait or migrate.
#[test]
fn math_coercion_waits_yields_collects_and_migrates() {
    for (expression, expected_order) in [
        ("Math.floor(v(-2.5, 152)) === -3", "152,"),
        ("Math.ceil.call(null, v(-2.5, 152)) === -2", "152,"),
        ("Math.abs.apply(null, [v(-2.5, 152)]) === 2.5", "152,"),
        ("Math.fround.bind(null, v(1.5, 152))() === 1.5", "152,"),
        ("Math.sqrt(v(4, 152)) === 2", "152,"),
        ("Math.log2.call(null, v(8, 152)) === 3", "152,"),
        ("Math.imul(v(-2, 152), v(4, 153)) === -8", "152,153,"),
        ("Math.pow(v(2, 152), v(3, 153)) === 8", "152,153,"),
        ("Math.pow(NaN, v(0, 152)) === 1", "152,"),
        ("Math.hypot(v(3, 152), v(4, 153)) === 5", "152,153,"),
        ("Math.hypot(NaN, Infinity, v(4, 152)) === Infinity", "152,"),
        (
            "Math.floor({n: -2.5, get valueOf() { order += '152,'; __rt.need(152, 1); return function() { return this.n; }; }}) === -3",
            "152,",
        ),
        (
            "Math.hypot(3, {get valueOf() { order += '152,'; __rt.need(152, 1); return () => ({}); }, get toString() { order += '153,'; __rt.need(153, 1); return () => '4'; }}) === 5",
            "152,153,",
        ),
        (
            "(() => { let caught = false; try { Math.hypot(Infinity, {get valueOf() { order += '152,'; __rt.need(152, 1); throw 42; }}); } catch (e) { caught = e === 42; } return caught && Math.floor(v(2.5, 153)) === 2; })()",
            "152,153,",
        ),
        (
            "Math.hypot({valueOf: Math.abs.bind(null, v(-3, 152))}, v(4, 153)) === 5",
            "152,153,",
        ),
        (
            "(() => { let caught = false; try { Math.hypot(Infinity, v(1, 152), {valueOf() { throw 42; }}); } catch (e) { caught = e === 42; } return caught && Math.floor(v(2.5, 153)) === 2; })()",
            "152,153,",
        ),
    ] {
        let rt = Runtime::new();
        let src = format!(
            "let order = ''; function v(n, id) {{ return {{n, valueOf() {{ order += id + ','; __rt.need(id, 1); return this.n; }} }}; }} let result = {expression}; result && order === '{expected_order}'"
        );
        let mut task = rt.spawn(&exe(&src), &[]).unwrap();
        let mut waits = 0;
        let mut complete = false;
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
                    waits += 1;
                    assert!(
                        waits <= expected_order.matches(',').count(),
                        "repeated wait: {expression}"
                    );
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
                RunOutcome::Complete(value) => {
                    assert_eq!(value.view(), ValueView::Bool(true), "{expression}");
                    complete = true;
                    break;
                }
                other => panic!("{expression}: {other:?}"),
            }
        }
        assert!(complete, "instruction bound exceeded: {expression}");
        assert_eq!(waits, expected_order.matches(',').count(), "{expression}");
    }
}

/// Synthetic fixtures, not light-cljs output. Four rows adapt QuickJS
/// test_builtin.js::test_number, revision 7d1c13c5d8f8. The full QuickJS
/// copyright and MIT notice is retained on string_code_unit_access below.
#[test]
fn number_to_string_with_radix() {
    for src in [
        r#"(1-2**-53).toString(12) === '0.bbbbbbbbbbbbbba'"#,
        r#"(1.3).toString(7) === '1.2046204620462046205'"#,
        r#"(1.3).toString(35) === '1.ahhhhhhhhhm'"#,
        r#"(1000000000000000128).toString() === '1000000000000000100'"#,
        r#"(10).toString(undefined) === '10' && (10).toString('10') === '10'"#,
        r#"(255).toString(16) === 'ff' && (-255).toString(2) === '-11111111'"#,
        r#"(0.5).toString(2) === '0.1' && (35).toString(36) === 'z'"#,
        r#"(new Number(31)).toString(16) === '1f' && Number.prototype.toString(2) === '0'"#,
        r#"(-0).toString(2) === '0' && NaN.toString(2) === 'NaN' && (-Infinity).toString(16) === '-Infinity'"#,
        r#"(10).toString(2.9) === '1010' && (10).toString({valueOf() {return 16;}}) === 'a'"#,
        r#"let log = ''; let radix = {valueOf() {log += 'v'; return {};}, toString() {log += 's'; return '2';}}; (10).toString(radix) === '1010' && log === 'vs'"#,
        r#"let touched = false; try {Number.prototype.toString.call('1', {valueOf() {touched = true; return 2;}});} catch(e) {} !touched"#,
        r#"let caught = false; try {(1).toString({valueOf() {throw 42;}});} catch(e) {caught = e === 42;} caught"#,
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
    for radix in [
        "1",
        "37",
        "NaN",
        "null",
        "false",
        "Infinity",
        "-Infinity",
        "0",
        "-2",
        "{}",
        "{valueOf() {return undefined;}}",
    ] {
        let src = format!(
            "let caught = false; try {{(10).toString({radix});}} catch(e) {{caught = true;}} caught"
        );
        assert_eq!(eval(&src), ValueView::Bool(true), "{src}");
        assert_eq!(
            failure(&format!("(10).toString({radix})")).kind(),
            VmErrorKind::RangeError
        );
    }
    assert_eq!(
        failure("Number.prototype.toString.call('1')").kind(),
        VmErrorKind::TypeError
    );
}

/// Synthetic fixture, not light-cljs output. Object radix coercion runs once
/// across a dependency wait, GC, one-instruction yields and worker migration.
#[test]
fn number_radix_coercion_waits_yields_collects_and_migrates() {
    for expression in [
        "n.toString(r) === 'ff'",
        "Number.prototype.toString.call(n, r) === 'ff'",
        "Number.prototype.toString.apply(n, [r]) === 'ff'",
        "Number.prototype.toString.bind(n)(r) === 'ff'",
        "n.toString({get valueOf() {count++; __rt.need(149, 1); return () => 16;}}) === 'ff'",
        "n.toString({valueOf() {return {};}, get toString() {count++; __rt.need(149, 1); return () => '16';}}) === 'ff'",
        "Number.prototype.toString.call(n, {valueOf: Number.bind(null, {valueOf() {count++; __rt.need(149, 1); return 2;}})}) === '11111111'",
        "(() => {try {n.toString({valueOf() {count++; __rt.need(149, 1); throw 42;}});} catch(e) {return e === 42;} return false;})()",
    ] {
        let rt = Runtime::new();
        let src = format!(
            "let count = 0; let n = new Number(255); let r = {{valueOf() {{count++; __rt.need(149, 1); return 16;}}}}; ({expression}) && count === 1"
        );
        let mut task = rt.spawn(&exe(&src), &[]).unwrap();
        let mut waits = 0;
        let mut complete = false;
        for _ in 0..500 {
            let (next, outcome) = std::thread::spawn(move || {
                let outcome = task
                    .run_with(Budget::instructions(1), &mut NoEffects)
                    .unwrap();
                (task, outcome)
            })
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
                    let value = rt.root(Primitive::Number(0.0));
                    task = rt
                        .complete(request.key(), request.producer(), Completion::Ready(&value))
                        .unwrap()
                        .into_runnable()
                        .pop()
                        .unwrap();
                }
                RunOutcome::Complete(result) => {
                    assert_eq!(result.view(), ValueView::Bool(true), "{src}");
                    complete = true;
                    break;
                }
                other => panic!("{src}: {other:?}"),
            }
        }
        assert!(complete, "instruction bound exceeded: {src}");
        assert_eq!(waits, 1);
    }
}

/// Synthetic fixtures, not light-cljs output (clj-quickjs-147).
/// The three invalid Number string conversions adapt test_number in QuickJS
/// tests/test_builtin.js, revision 7d1c13c5d8f8. The full QuickJS copyright
/// and MIT permission notice is retained on string_code_unit_access below.
#[test]
fn number_predicates_and_constants() {
    for src in [
        r#"isNaN(NaN) && isNaN('abc') && !isNaN('12') && isNaN(undefined) && isNaN()"#,
        r#"isFinite('1e3') && !isFinite(Infinity) && !isFinite(-Infinity) && !isFinite(NaN) && !isFinite()"#,
        r#"!isNaN(null) && !isNaN(true) && isFinite(null) && isFinite(false) && isFinite('')"#,
        r#"!Number.isNaN('abc') && Number.isNaN(NaN) && !Number.isNaN(undefined) && !Number.isNaN()"#,
        r#"Number.isNaN(Number('+')) && Number.isNaN(Number('-')) && Number.isNaN(Number('\x00a'))"#,
        r#"Number.isFinite(0) && Number.isFinite(-0) && Number.isFinite(Number.MAX_VALUE) && !Number.isFinite('1e3') && !Number.isFinite(null) && !Number.isFinite(undefined) && !Number.isFinite(Infinity) && !Number.isFinite(-Infinity) && !Number.isFinite(NaN) && !Number.isFinite()"#,
        r#"Number.isInteger(5.0) && !Number.isInteger(5.5) && Number.isInteger(-0) && Number.isInteger(-5) && Number.isInteger(Number.MAX_VALUE) && !Number.isInteger(NaN) && !Number.isInteger(Infinity) && !Number.isInteger(-Infinity) && !Number.isInteger('5') && !Number.isInteger()"#,
        r#"!Number.isSafeInteger(2**53) && Number.isSafeInteger(2**53 - 1) && Number.isSafeInteger(-(2**53 - 1)) && !Number.isSafeInteger(-(2**53)) && Number.isSafeInteger(-0) && !Number.isSafeInteger(5.5) && !Number.isSafeInteger(NaN) && !Number.isSafeInteger(Infinity) && !Number.isSafeInteger('5') && !Number.isSafeInteger()"#,
        r#"Number.EPSILON === 2**-52 && Number.EPSILON / 2 + 1 === 1 && Number.MIN_VALUE === 5e-324 && Number.MIN_VALUE / 2 === 0 && Number.MAX_VALUE === 1.7976931348623157e308 && Number.MAX_VALUE * 2 === Infinity"#,
        r#"Number.MAX_SAFE_INTEGER === 9007199254740991 && Number.MIN_SAFE_INTEGER === -9007199254740991 && Number.NaN !== Number.NaN && Number.POSITIVE_INFINITY === Infinity && Number.NEGATIVE_INFINITY === -Infinity"#,
        r#"let o = {valueOf() { throw 1; }, toString() { throw 2; }}; !Number.isNaN(o) && !Number.isFinite(o) && !Number.isInteger(o) && !Number.isSafeInteger(o) && !Number.isFinite(new Number(1)) && !Number.isNaN(new Number(NaN))"#,
        r#"let log = ''; let o = {valueOf() { log += 'v'; return {}; }, toString() { log += 's'; return 'abc'; }}; isNaN(o) && !isFinite(o) && log === 'vsvs'"#,
        r#"let o = {n: 7, valueOf() { return this.n; }}; !isNaN(o) && isFinite(o) && isFinite(new Number(2)) && isNaN(new String('abc'))"#,
        r#"let caught = false; try { isNaN({valueOf() { throw 42; }}); } catch (e) { caught = e === 42; } caught"#,
        r#"let caught = false; try { isFinite({valueOf() { return {}; }, toString() { return {}; }}); } catch (e) { caught = true; } caught"#,
        r#"isNaN.name === 'isNaN' && isFinite.name === 'isFinite' && isNaN.length === 1 && isFinite.length === 1 && Number.isNaN.name === 'isNaN' && Number.isFinite.name === 'isFinite' && Number.isInteger.name === 'isInteger' && Number.isSafeInteger.name === 'isSafeInteger' && Number.isNaN.length === 1 && Number.isFinite.length === 1 && Number.isInteger.length === 1 && Number.isSafeInteger.length === 1"#,
        r#"let isNaN = function () { return 7; }; let isFinite = 8; isNaN() === 7 && isFinite === 8"#,
        r#"isNaN.call(null, 'abc') && isFinite.apply(null, ['7']) && Number.isInteger.call(null, 7) && Number.isSafeInteger.bind(null)(7)"#,
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
    for name in [
        "EPSILON",
        "MIN_VALUE",
        "MAX_VALUE",
        "MAX_SAFE_INTEGER",
        "MIN_SAFE_INTEGER",
        "POSITIVE_INFINITY",
        "NEGATIVE_INFINITY",
        "NaN",
    ] {
        let src = format!(
            "let d = Object.getOwnPropertyDescriptor(Number, '{name}'); !d.writable && !d.enumerable && !d.configurable && Object.is(d.value, Number['{name}'])"
        );
        assert_eq!(eval(&src), ValueView::Bool(true), "{name}");
    }
    assert_eq!(
        eval(
            r#"'use strict'; let caught = false; try { Number.EPSILON = 1; } catch (e) { caught = true; } caught && Number.EPSILON === 2**-52"#
        ),
        ValueView::Bool(true)
    );
    assert_eq!(
        failure("'use strict'; Number.EPSILON = 1").kind(),
        VmErrorKind::TypeError
    );
    assert_eq!(
        failure("isFinite(Object.create(null))").kind(),
        VmErrorKind::TypeError
    );
    for src in ["new isNaN(1)", "new isFinite(1)", "new Number.isInteger(1)"] {
        assert_eq!(failure(src).kind(), VmErrorKind::TypeError, "{src}");
    }
}

/// Synthetic fixtures, not light-cljs output (clj-quickjs-261).
#[test]
fn stored_proto_accessor() {
    let descriptor = "let d = Object.getOwnPropertyDescriptor(Object.prototype, '__proto__');";
    for src in [
        "d.get.length === 0 && d.set.length === 1 && !('prototype' in d.get) && !('prototype' in d.set) && Object.getPrototypeOf(d.get) === Function.prototype && d.get === Object.getOwnPropertyDescriptor(Object.prototype, '__proto__').get",
        "d.get.call([]) === Array.prototype && d.get.call({}) === Object.prototype && d.get.call(Object.create(null)) === null && d.get.call(Object.prototype) === null",
        "d.get.call(1) === Number.prototype && d.get.call('abc') === String.prototype && d.get.call(new Number(2)) === Number.prototype && d.get.call(new String('a')) === String.prototype",
        "d.get.apply([], []) === Array.prototype && d.get.bind(1)() === Number.prototype && Reflect.apply(d.get, Object.prototype, []) === null",
        "let b = true; b.__proto__ = {}; (1).__proto__ = null; 'a'.__proto__ = {}; b === true",
        "let o = {}; let p = {x: 7}; d.set.call(o, p) === undefined && o.x === 7 && d.get.call(o) === p && d.set.call(o, null) === undefined && d.get.call(o) === null",
        "let o = {}; let p = d.get.call(o); let v = {valueOf() { throw 7; }, toString() { throw 8; }}; d.set.call(o) === undefined && d.set.call(o, 1) === undefined && d.set.call(1, v) === undefined && d.set.call('a', null) === undefined && d.set.call(true, v) === undefined && d.get.call(o) === p",
        "let o = {}; Object.defineProperty(o, 'link', d); let p = {}; (o.link = p) === p && o.link === p && Object.getPrototypeOf(o) === p",
        "let p = {}; Object.defineProperty(p, 'link', d); let o = Object.create(p); let q = {}; (o.link = q) === q && Object.getPrototypeOf(o) === q && Object.getOwnPropertyDescriptor(o, 'link') === undefined && Object.getPrototypeOf(p) === Object.prototype",
        "let a = {x: 3, __proto__: null}; let o = {__proto__: a}; !('__proto__' in o) && Object.getOwnPropertyDescriptor(o, '__proto__') === undefined && o.x === 3",
        "Object.keys(Object.prototype).length === 0 && '__proto__' in {} && !('__proto__' in Object.create(null))",
    ] {
        let src = format!("{descriptor} {src}");
        assert_eq!(eval(&src), ValueView::Bool(true), "{src}");
    }
    for src in [
        "d.get.call(null)",
        "d.get.call(undefined)",
        "d.set.call(null, 1)",
        "d.set.call(undefined)",
        "d.set.call(Object.prototype, {})",
        "d.set.call(Object.preventExtensions({}), {})",
        "let o = {}; d.set.call(o, o)",
        "new d.get()",
        "new d.set({})",
        "Object.defineProperty(Object.prototype, '__proto__', {get: d.get})",
        "delete Object.prototype.__proto__",
        "d.get.toString = 1",
        "d.set.caller = 1",
    ] {
        let src = format!("{descriptor} {src}");
        assert_eq!(failure(&src).kind(), VmErrorKind::TypeError, "{src}");
        assert_eq!(
            sloppy_eval(&format!(
                "let caught = false; try {{ {src}; }} catch (e) {{ caught = true; }} caught"
            )),
            ValueView::Bool(true),
            "{src}"
        );
    }
}

#[test]
fn accessor_properties() {
    // Synthetic fixtures, not light-cljs output.
    for src in [
        "let a = {get z() { return 4; }, set z(v) { this.z_val = v; }}; let read = a.z; a.z = 5; read === 4 && a.z_val === 5",
        "let a = {}; let setter = function(v) { this.z_val = v; }; Object.defineProperty(a, 'z', {get: function() { return 4; }, set: setter, configurable: true, enumerable: true}); let read = a.z; a.z = 5; read === 4 && a.z_val === 5 && Object.getOwnPropertyDescriptor(a, 'z').set === setter",
        "let a = {get: 2, set: 3, async: 4, get a() { return this.get; }}; a.a === 2",
        "let p = {get z() { return this.x; }, set z(v) { this.x = v; }}; let a = Object.create(p); a.x = 7; let read = a.z; a.z = 8; read === 7 && a.x === 8 && Object.getOwnPropertyDescriptor(a, 'z') === undefined",
        "let a = {get z() { return 4; }}; let caught = false; try { a.z = 5; } catch(e) { caught = true; } caught && a.z === 4",
        "let a = {get z() { throw 19; }}; let result = 0; try { a.z; } catch(e) { result = e; } result === 19",
        "let a = {get z() { return 4; }, set z(v) { this.x = v; }}; let d = Object.getOwnPropertyDescriptor(a, 'z'); d.get.name === 'get z' && d.set.name === 'set z' && d.enumerable && d.configurable && !('value' in d) && !('writable' in d) && !('prototype' in d.get)",
        "let a = {get z() { return 1; }, z: 9}; a.z === 9 && Object.getOwnPropertyDescriptor(a, 'z').value === 9",
        "let a = {z: 1, set z(v) { this.x = v; }}; a.z === undefined && (a.z = 9) === 9 && a.x === 9",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
}

#[test]
fn accessor_descriptors_and_integrity() {
    // Synthetic fixtures, not light-cljs output.
    for src in [
        r"let o = {get \u0069f() { return this.x; }, set \u0069f(v) { this.x = v; }}; o.if = 7; o.if === 7 && Object.getOwnPropertyDescriptor(o, 'if').get.name === 'get if'",
        "let a = {get 'x'() { return 2; }, set 1(v) { this.x1 = v; }, get 1() { return this.x1; }}; a[1] = 3; a.x === 2 && a[1] === 3",
        "let a = {get async() { return 3; }, get if() { return 4; }, get get() { return 5; }, get set() { return 6; }}; a.async + a.if + a.get + a.set === 18",
        "let a = {get __proto__() { return 3; }, set __proto__(v) { this.x = v; }}; a.__proto__ = 4; a.__proto__ === 3 && a.x === 4 && Object.getPrototypeOf(a) === Object.prototype",
        "let a = {get z() { return 1; }, set z(v) { this.x = v; }, get z() { return 2; }}; a.z = 3; a.z === 2 && a.x === 3",
        "let a = {set z(v) { this.x = v; }, z: 1, get z() { return 2; }}; Object.getOwnPropertyDescriptor(a, 'z').set === undefined && a.z === 2",
        "let get = () => 1; let a = {}; Object.defineProperty(a, 'z', {get: get}); Object.defineProperty(a, 'z', {get: get}); let caught = false; try { Object.defineProperty(a, 'z', {get: () => 2}); } catch(e) { caught = true; } caught && a.z === 1",
        "let a = {get z() { return 1; }}; Object.defineProperty(a, 'z', {value: 3}); let d = Object.getOwnPropertyDescriptor(a, 'z'); a.z === 3 && !d.writable && d.enumerable && d.configurable && !('get' in d)",
        "let a = {z: 1}; Object.defineProperty(a, 'z', {get: () => 3}); let d = Object.getOwnPropertyDescriptor(a, 'z'); a.z === 3 && d.set === undefined && d.enumerable && d.configurable && !('value' in d)",
        "let n = 0; let a = {get z() { n++; return 1; }}; let names = Object.keys(a); let listed = ''; for (let k in a) { listed += k; } let has = 'z' in a; let d = Object.getOwnPropertyDescriptor(a, 'z'); let removed = delete a.z; n === 0 && has && removed && listed === 'z' && names[0] === 'z' && d.get.name === 'get z' && !('z' in a)",
        "let n = 0; let a = {}; Object.defineProperty(a, 'z', {get: () => { n++; return 1; }}); let names = Object.keys(a); let listed = ''; for (let k in a) { listed += k; } names.length === 0 && listed === '' && ('z' in a) && n === 0",
        "let x = 0; let a = {get z() { return x; }, set z(v) { x = v; }}; Object.freeze(a); a.z = 9; let d = Object.getOwnPropertyDescriptor(a, 'z'); Object.isFrozen(a) && a.z === 9 && !d.configurable && d.set !== undefined",
        "let a = []; Object.defineProperty(a, 3, {get: () => 8, set: function(v) { this.x = v; }, configurable: true, enumerable: true}); let names = Object.keys(a); a[3] = 7; let ok = a.length === 4 && a[3] === 8 && a.x === 7 && names[0] === '3'; delete a[3]; ok && a.length === 4 && !(3 in a)",
        "let x = 0; let a = []; Object.defineProperty(a, 0, {set: function(v) { x = v; }, configurable: true}); Object.freeze(a); a[0] = 4; Object.isFrozen(a) && a[0] === undefined && x === 4",
        "let a = []; Object.defineProperty(a, 2, {get: () => 7, configurable: true}); a.length = 0; a.length === 0 && !(2 in a)",
        "let a = []; Object.defineProperty(a, 2, {get: () => 7}); let caught = false; try { a.length = 0; } catch(e) { caught = true; } caught && a.length === 3 && a[2] === 7",
        "let a = {get z() { return 8; }, set z(v) { this.x = v; }}; let b = Object.create(a); Object.preventExtensions(b); let caught = false; try { b.z = 3; } catch(e) { caught = true; } caught && !('x' in b) && b.z === 8",
        "let o = {get z() { return 7; }}; let d = Object.getOwnPropertyDescriptor(o, 'z'); let caught = false; try { new d.get(); } catch(e) { caught = true; } caught && d.get.length === 0",
        "let order = ''; let d = {get enumerable() { order += 'e'; return 1; }, get configurable() { order += 'c'; return 1; }, get value() { order += 'v'; return 7; }, get writable() { order += 'w'; return 1; }}; let o = {}; Object.defineProperty(o, 'z', d); order === 'ecvw' && o.z === 7",
        "let order = ''; let d = {get get() { order += 'g'; return 1; }, get set() { order += 's'; return undefined; }}; let caught = false; try { Object.defineProperty({}, 'z', d); } catch(e) { caught = true; } caught && order === 'g'",
        "let p = {get value() { return this.x; }}; let d = Object.create(p); d.x = 9; let a = {}; Object.defineProperty(a, 'z', d); a.z === 9",
        "let f = () => 0; Object.defineProperty(f, 'name', {get: function() { return this.x; }}); f.x = 'F'; f.name === 'F' && Object.getOwnPropertyDescriptor(f, 'name').get !== undefined && ('name' in f)",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
    assert_eq!(
        sloppy_eval("let o = {get x() { return 1; }}; o.x = 2; o.x"),
        ValueView::Number(1.0)
    );
}

#[test]
fn accessors_in_native_operations() {
    // Synthetic fixtures, not light-cljs output.
    for src in [
        "let o = {x: 9}; let f = function() { return this.x; }; Object.defineProperty(o, 'z', {get: f.bind({x: 7})}); o.z === 7",
        "let o = {}; Object.defineProperty(o, 'z', {get: Object.prototype.valueOf}); o.z === o",
        "let p = {}; let f = () => 0; Object.defineProperty(f, 'prototype', {get: () => p}); let o = Object.create(p); o instanceof f && !(1 instanceof f)",
        "let f = () => 0; Object.defineProperty(f, 'prototype', {get: () => { throw 7; }}); let caught = 0; try { ({}) instanceof f; } catch(e) { caught = e; } caught === 7 && !(1 instanceof f)",
        "let f = (a, b) => a + b; Object.defineProperty(f, 'length', {get: () => 4}); Object.defineProperty(f, 'name', {get: () => 'sum'}); let b = f.bind(null, 2); b.length === 3 && b.name === 'bound sum' && b(3) === 5",
        "let order = ''; let args = {get length() { order += 'l'; return 2; }, get 0() { order += 'a'; return 3; }, get 1() { order += 'b'; return 4; }}; let f = function(a, b) { return this.x + a + b; }; f.apply({x: 2}, args) === 9 && order === 'lab'",
        "let args = {get length() { return {valueOf: () => 2}; }, get 0() { return 3; }, get 1() { return 4; }}; Reflect.apply((a, b) => a + b, null, args) === 7",
        "let p = {set 0(v) { this.x = v; }}; let a = Object.create(p); a.length = 0; Array.prototype.push.call(a, 7) === 1 && a.x === 7 && a.length === 1 && Object.getOwnPropertyDescriptor(a, 0) === undefined",
        "let order = ''; let a = {get length() { order += 'g'; return 0; }, set length(v) { order += 's'; this.len = v; }, set 0(v) { order += 'a'; this.x = v; }}; Array.prototype.push.call(a, 7) === 1 && a.x === 7 && a.len === 1 && order === 'gas'",
        "let o = {get valueOf() { return () => 7; }}; +o === 7",
        "let o = {get valueOf() { return 0; }, get toString() { return () => '8'; }}; +o === 8",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
}

/// String access cases adapted from QuickJS tests/test_builtin.js,
/// test_string and rope_concat, revision 7d1c13c5d8f8.
/// Synthetic fixtures, not light-cljs output.
/// Copyright (c) 2017-2021 Fabrice Bellard.
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
fn string_code_unit_access() {
    for src in [
        r#"let a = String('abc'); a.length === 3 && a[1] === 'b' && a.charCodeAt(1) === 0x62 && a.charAt(1) === 'b' && a.charAt(-1) === '' && a.charAt(3) === ''"#,
        r#"String.fromCharCode(65) === 'A' && String.fromCharCode.apply(null, [65, 66, 67]) === 'ABC'"#,
        r#"let a = String.fromCharCode(0x20ac); a === '€' && a.charCodeAt(0) === 0x20ac"#,
        r#"let a = '\u{10ffff}'; a.codePointAt(0) === 0x10ffff && String.fromCodePoint(0x10ffff) === a && a.codePointAt(1) === 0xdfff && a.codePointAt(2) === undefined"#,
        r#"'abc'.charCodeAt() === 0x61 && Object.is('abc'.charCodeAt(3), NaN) && 'abc'.charAt(NaN) === 'a'"#,
        r#"String.fromCharCode() === '' && String.fromCodePoint() === '' && String.fromCharCode(0x10041) === 'A'"#,
        r#"String.fromCharCode(0xd800).length === 1 && String.fromCharCode(0xd800) === '\ud800' && String.fromCharCode(0xd800).charCodeAt(0) === 0xd800 && String.fromCodePoint(0x41, 0x1f600).length === 3 && String.fromCodePoint(0x41, 0x1f600) === 'A\ud83d\ude00'"#,
        r#"'abc'.charAt(1.9) === 'b' && 'abc'.charAt(-0.9) === 'a' && 'abc'.charAt('2') === 'c' && 'abc'.charCodeAt(null) === 97 && 'abc'.codePointAt(true) === 98"#,
        r#"'abc'.charAt(Infinity) === '' && 'abc'.charAt(-Infinity) === '' && Object.is('abc'.charCodeAt(-1), NaN) && 'abc'.codePointAt(-1) === undefined && 'abc'.charAt(4294967296) === ''"#,
        r#"String.fromCharCode(-1, 65536, 65.9, NaN, Infinity, undefined, null, true, '66') === '\uffff\0A\0\0\0\0\u0001B'"#,
        r#"String.fromCodePoint('65', null, true, -0, 0xd800) === 'A\0\u0001\0\ud800'"#,
        r#"'\ud800x'.codePointAt(0) === 0xd800 && '\udc00\ud800'.codePointAt(0) === 0xdc00 && '\ud800'.codePointAt(0) === 0xd800 && '\ud800\udc00'.charAt(0) === '\ud800'"#,
        r#"String.prototype.charAt.call(123, 1) === '2' && String.prototype.charCodeAt.call(false, 0) === 102 && String.prototype.charAt() === ''"#,
        r#"String.prototype.charAt.length === 1 && String.prototype.charCodeAt.length === 1 && String.prototype.codePointAt.length === 1 && String.fromCharCode.length === 1 && String.fromCodePoint.length === 1"#,
        // Exercise every UTF-16 unit without quadratic rope construction.
        r#"let ok = true; for (let i = 0; i < 65536; i++) { let s = String.fromCharCode(i & 0xffff); if (s.charCodeAt(0) !== i || s.codePointAt(0) !== i || s.charCodeAt(1) === s.charCodeAt(1)) { ok = false; break; } } ok"#,
        r#"let units = []; for (let i = 0; i < 65536; i++) { units.push(i); } let s = String.fromCharCode.apply(null, units); let ok = true; for (let i = 0; i < 65536; i++) { if (s.charCodeAt(i) !== (i & 0xffff)) { ok = false; break; } } ok"#,
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
    for (src, kind) in [
        ("String.fromCodePoint(0x110000)", VmErrorKind::RangeError),
        ("String.fromCodePoint(1.5)", VmErrorKind::RangeError),
        ("String.fromCodePoint(-1)", VmErrorKind::RangeError),
        ("String.fromCodePoint(NaN)", VmErrorKind::RangeError),
        ("String.fromCodePoint(Infinity)", VmErrorKind::RangeError),
        ("String.fromCodePoint(undefined)", VmErrorKind::RangeError),
        ("String.prototype.charAt.call(null)", VmErrorKind::TypeError),
        (
            "String.prototype.charCodeAt.call(undefined)",
            VmErrorKind::TypeError,
        ),
        (
            "String.prototype.codePointAt.call(null)",
            VmErrorKind::TypeError,
        ),
    ] {
        assert_eq!(failure(src).kind(), kind, "{src}");
        assert_eq!(
            eval(&format!(
                "let caught = false; try {{ {src}; }} catch(e) {{ caught = true; }} caught"
            )),
            ValueView::Bool(true),
            "{src}"
        );
    }
}

#[test]
fn string_code_unit_coercion() {
    // Synthetic fixtures, not light-cljs output.
    for src in [
        "'abc'.charAt({valueOf: () => 1}) === 'b'",
        "String.fromCharCode({valueOf: () => 65}) === 'A'",
        "String.fromCodePoint({valueOf: () => 65}) === 'A'",
        "String.prototype.charAt.call({toString: () => 'abc'}, 0) === 'a'",
        "new String('abc').charAt(1) === 'b' && new String('abc').charCodeAt(2) === 99 && new String('😀').codePointAt(0) === 0x1f600",
        "String.fromCharCode(new Number(65)) === 'A' && String.fromCodePoint(new String('128512')) === '😀'",
        "let order = ''; let receiver = {toString: () => {order += 's'; return 123; }}; let pos = {valueOf: () => {order += 'p'; return 1; }}; String.prototype.charAt.call(receiver, pos) === '2' && order === 'sp'",
        "let order = ''; let receiver = {toString: () => {order += 's'; receiver.valueOf = () => {order += 'v'; return 'abc'; }; return {}; }}; String.prototype.charCodeAt.call(receiver, 1) === 98 && order === 'sv'",
        "let calls = 0; let pos = {valueOf: () => {calls++; return 0; }}; let caught = false; try { String.prototype.charAt.call(null, pos); } catch (e) { caught = true; } caught && calls === 0",
        "let calls = 0; let later = {valueOf: () => {calls++; return 65; }}; let caught = false; try { String.fromCodePoint(0x110000, later); } catch (e) { caught = true; } caught && calls === 0",
        "let order = ''; let bad = {valueOf: () => {order += 'a'; return 1.5; }}; let later = {valueOf: () => {order += 'b'; return 65; }}; let caught = false; try { String.fromCodePoint(bad, later); } catch (e) { caught = true; } caught && order === 'a'",
        "let calls = 0; let bad = {valueOf: () => {calls++; throw 42; }}; let caught = 0; try { String.fromCharCode(bad, bad); } catch (e) { caught = e; } caught === 42 && calls === 1",
        "let bad = {toString: () => ({}), valueOf: () => ({})}; let caught = false; try { String.prototype.codePointAt.call(bad); } catch (e) { caught = true; } caught",
        "let f = String.fromCharCode.bind(null, {valueOf: () => 65}); f.apply(null, [66]) === 'AB' && Reflect.apply(String.fromCodePoint, null, [{valueOf: () => 0x1f600}]) === '😀'",
        // A conversion method that is itself a built-in needs a nested native.
        "let receiver = {toString: String.fromCharCode.bind(null, {valueOf: () => 65})}; String.prototype.charAt.call(receiver, 0) === 'A'",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
}

/// Synthetic fixtures, not light-cljs output. Both the receiver and later
/// arguments must survive while a callback waits, without repeating effects.
#[test]
fn string_code_unit_coercion_waits_yields_collects_and_migrates() {
    for (expression, expected_order, expected_waits) in [
        (
            "String.prototype.charAt.call(receiver(), position()) === 'b'",
            "sp",
            2,
        ),
        (
            "String.prototype.charCodeAt.apply(receiver(), [position()]) === 98",
            "sp",
            2,
        ),
        (
            "String.prototype.indexOf.call(receiver(), search(), position()) === 1",
            "sbp",
            3,
        ),
        (
            "Reflect.apply(String.prototype.lastIndexOf, receiver(), [search(), position()]) === 1",
            "sbp",
            3,
        ),
        (
            "String.prototype.split.call(receiver(), search(), position()).join('|') === 'a'",
            "spb",
            3,
        ),
        (
            "String.prototype.split.bind(receiver(), search()).apply(null, [code(0, 144)]).length === 0",
            "snb",
            3,
        ),
        (
            "String.prototype.substring.call(receiver(), position(), code(2, 145)) === 'b'",
            "spn",
            3,
        ),
        (
            "String.prototype.slice.apply(receiver(), [position(), code(2, 145)]) === 'b'",
            "spn",
            3,
        ),
        (
            "String.prototype.concat.call(receiver(), search(), search()) === 'abcbb'",
            "sbb",
            2,
        ),
        (
            "String.prototype.padStart.call(receiver(), code(5, 144), search()) === 'bbabc'",
            "snb",
            3,
        ),
        (
            "Reflect.apply(String.prototype.padEnd, receiver(), [code(5, 144), search()]) === 'abcbb'",
            "snb",
            3,
        ),
        (
            "Reflect.apply(String.prototype.codePointAt, receiver(), [position()]) === 98",
            "sp",
            2,
        ),
        (
            "String.fromCharCode(code(65, 143), code(66, 144)) === 'AB'",
            "nn",
            2,
        ),
        (
            "String.fromCodePoint(code(65, 143), code(0x1f600, 144)) === 'A😀'",
            "nn",
            2,
        ),
        (
            "String.prototype.charAt.call({toString: String.fromCharCode.bind(null, code(65, 143))}, 0) === 'A'",
            "n",
            1,
        ),
        (
            "(() => { let caught = 0; try { String.fromCodePoint({valueOf: () => {order += 'n'; __rt.need(143, 1); throw 42; }}, code(65, 144)); } catch (e) { caught = e; } return caught === 42; })()",
            "n",
            1,
        ),
    ] {
        let rt = Runtime::new();
        let src = format!(
            "let order = '';
             function receiver() {{ return {{toString: function () {{ order += 's'; __rt.need(143, 1); return 'abc'; }} }}; }}
             function position() {{ return {{valueOf: function () {{ order += 'p'; __rt.need(144, 1); return 1; }} }}; }}
             function search() {{ return {{toString: function () {{ order += 'b'; __rt.need(145, 1); return 'b'; }} }}; }}
             function code(n, key) {{ return {{n: n, key: key, valueOf: function () {{ order += 'n'; __rt.need(this.key, 1); return this.n; }} }}; }}
             let ok = {expression}; ok && order === '{expected_order}'"
        );
        let mut task = rt.spawn(&exe(&src), &[]).unwrap();
        let mut waits = 0;
        let mut complete = false;
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
                    waits += 1;
                    assert!(waits <= expected_waits, "repeated wait: {expression}");
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
        assert!(complete, "instruction bound exceeded: {expression}");
        assert_eq!(waits, expected_waits, "{expression}");
    }
}

#[test]
fn string_code_unit_native_recursion_obeys_depth_limit() {
    let rt = Runtime::with_limits(clj_quickjs::VmLimits {
        max_call_depth: 8,
        ..clj_quickjs::VmLimits::DEFAULT
    });
    let script =
        exe("let o = {}; o.valueOf = String.fromCharCode.bind(null, o); String.fromCharCode(o)");
    match rt
        .spawn(&script, &[])
        .unwrap()
        .run_with(Budget::instructions(1000), &mut NoEffects)
        .unwrap()
    {
        RunOutcome::Failed(e) => assert!(matches!(e.kind(), VmErrorKind::StackOverflow)),
        other => panic!("native recursion did not unwind: {other:?}"),
    }
}

/// Synthetic fixtures, not light-cljs output.
#[test]
fn string_code_unit_method_descriptors() {
    for (owner, name) in [
        ("String.prototype", "charAt"),
        ("String.prototype", "charCodeAt"),
        ("String.prototype", "codePointAt"),
        ("String", "fromCharCode"),
        ("String", "fromCodePoint"),
    ] {
        let src = format!(
            "let owner = {owner}; let d = Object.getOwnPropertyDescriptor(owner, '{name}'); \
             d.value === owner['{name}'] && typeof d.value === 'function' && \
             d.writable === true && d.enumerable === false && d.configurable === true && \
             d.value.name === '{name}' && d.value.length === 1"
        );
        assert_eq!(eval(&src), ValueView::Bool(true), "{src}");
    }
    assert_eq!(
        eval("Object.keys(String.prototype).length === 0 && Object.keys(String).length === 0"),
        ValueView::Bool(true)
    );
}

/// Search cases adapted from QuickJS tests/test_builtin.js, test_string,
/// revision 7d1c13c5d8f8, under the full QuickJS copyright and MIT permission
/// notice on string_code_unit_access above. Synthetic fixtures, not light-cljs output.
#[test]
fn string_index_of_and_last_index_of() {
    for (src, expected) in [
        (r#""abcabc".indexOf("cab")"#, 2.0),
        (r#""abcabc".indexOf("cab2")"#, -1.0),
        (r#""abc".indexOf("c")"#, 2.0),
        (r#""aaa".indexOf("a")"#, 0.0),
        (r#""aaa".indexOf("a", NaN)"#, 0.0),
        (r#""aaa".indexOf("a", -Infinity)"#, 0.0),
        (r#""aaa".indexOf("a", -1)"#, 0.0),
        (r#""aaa".indexOf("a", -0)"#, 0.0),
        (r#""aaa".indexOf("a", 0)"#, 0.0),
        (r#""aaa".indexOf("a", 1)"#, 1.0),
        (r#""aaa".indexOf("a", 2)"#, 2.0),
        (r#""aaa".indexOf("a", 3)"#, -1.0),
        (r#""aaa".indexOf("a", 4)"#, -1.0),
        (r#""aaa".indexOf("a", Infinity)"#, -1.0),
        (r#""aaa".indexOf("")"#, 0.0),
        (r#""aaa".indexOf("", NaN)"#, 0.0),
        (r#""aaa".indexOf("", -Infinity)"#, 0.0),
        (r#""aaa".indexOf("", -1)"#, 0.0),
        (r#""aaa".indexOf("", -0)"#, 0.0),
        (r#""aaa".indexOf("", 0)"#, 0.0),
        (r#""aaa".indexOf("", 1)"#, 1.0),
        (r#""aaa".indexOf("", 2)"#, 2.0),
        (r#""aaa".indexOf("", 3)"#, 3.0),
        (r#""aaa".indexOf("", 4)"#, 3.0),
        (r#""aaa".indexOf("", Infinity)"#, 3.0),
        (r#""aaa".lastIndexOf("a")"#, 2.0),
        (r#""aaa".lastIndexOf("a", NaN)"#, 2.0),
        (r#""aaa".lastIndexOf("a", -Infinity)"#, 0.0),
        (r#""aaa".lastIndexOf("a", -1)"#, 0.0),
        (r#""aaa".lastIndexOf("a", -0)"#, 0.0),
        (r#""aaa".lastIndexOf("a", 0)"#, 0.0),
        (r#""aaa".lastIndexOf("a", 1)"#, 1.0),
        (r#""aaa".lastIndexOf("a", 2)"#, 2.0),
        (r#""aaa".lastIndexOf("a", 3)"#, 2.0),
        (r#""aaa".lastIndexOf("a", 4)"#, 2.0),
        (r#""aaa".lastIndexOf("a", Infinity)"#, 2.0),
        (r#""aaa".lastIndexOf("")"#, 3.0),
        (r#""aaa".lastIndexOf("", NaN)"#, 3.0),
        (r#""aaa".lastIndexOf("", -Infinity)"#, 0.0),
        (r#""aaa".lastIndexOf("", -1)"#, 0.0),
        (r#""aaa".lastIndexOf("", -0)"#, 0.0),
        (r#""aaa".lastIndexOf("", 0)"#, 0.0),
        (r#""aaa".lastIndexOf("", 1)"#, 1.0),
        (r#""aaa".lastIndexOf("", 2)"#, 2.0),
        (r#""aaa".lastIndexOf("", 3)"#, 3.0),
        (r#""aaa".lastIndexOf("", 4)"#, 3.0),
        (r#""aaa".lastIndexOf("", Infinity)"#, 3.0),
        (r#""a\u{10000}b".indexOf("b")"#, 3.0),
        (r#""x".indexOf()"#, -1.0),
    ] {
        assert_eq!(eval(src), ValueView::Number(expected), "{src}");
    }
    let src = "String.prototype.indexOf.call(null, 'a')";
    assert_eq!(failure(src).kind(), VmErrorKind::TypeError);
    assert_eq!(
        eval(&format!(
            "let caught = false; try {{ {src}; }} catch (e) {{ caught = true; }} caught"
        )),
        ValueView::Bool(true)
    );
}

/// Split rows adapted from QuickJS tests/test_builtin.js, test_string,
/// revision 7d1c13c5d8f8, under the full QuickJS copyright and MIT permission
/// notice on string_code_unit_access above. Synthetic fixtures, not light-cljs output.
#[test]
fn string_split() {
    for (expression, expected) in [
        (r#""a,b,c".split(",")"#, vec!["a", "b", "c"]),
        (r#"",b,c".split(",")"#, vec!["", "b", "c"]),
        (r#""a,b,".split(",")"#, vec!["a", "b", ""]),
        (r#""aaaa".split()"#, vec!["aaaa"]),
        (r#""aaaa".split(undefined, 0)"#, vec![]),
        (r#""aaaa".split("")"#, vec!["a", "a", "a", "a"]),
        (r#""aaaa".split("", 0)"#, vec![]),
        (r#""aaaa".split("", 1)"#, vec!["a"]),
        (r#""aaaa".split("", 2)"#, vec!["a", "a"]),
        (r#""aaaa".split("a")"#, vec!["", "", "", "", ""]),
        (r#""aaaa".split("a", 2)"#, vec!["", ""]),
        (r#""aaaa".split("aa")"#, vec!["", "", ""]),
        (r#""aaaa".split("aa", 0)"#, vec![]),
        (r#""aaaa".split("aa", 1)"#, vec![""]),
        (r#""aaaa".split("aa", 2)"#, vec!["", ""]),
        (r#""aaaa".split("aaa")"#, vec!["", "a"]),
        (r#""aaaa".split("aaaa")"#, vec!["", ""]),
        (r#""aaaa".split("aaaaa")"#, vec!["aaaa"]),
        (r#""aaaa".split("aaaaa", 0)"#, vec![]),
        (r#""aaaa".split("aaaaa", 1)"#, vec!["aaaa"]),
        (r#""".split("")"#, vec![]),
        (r#""".split(",")"#, vec![""]),
        (r#""a,b".split(",", -1)"#, vec!["a", "b"]),
        (r#""a1b1c".split(1)"#, vec!["a", "b", "c"]),
    ] {
        let elements = expected
            .iter()
            .enumerate()
            .map(|(i, value)| format!(" && a[{i}] === {value:?}"))
            .collect::<String>();
        let src = format!(
            "let a = {expression}; Object.getPrototypeOf(a) === Array.prototype && a.length === {}{elements} && a.join('|') === {:?}",
            expected.len(),
            expected.join("|")
        );
        assert_eq!(eval(&src), ValueView::Bool(true), "{expression}");
    }
    assert_eq!(
        eval(r#""\u{10000}".split("").length"#),
        ValueView::Number(2.0)
    );
    assert_eq!(
        failure("String.prototype.split.call(null, ',')").kind(),
        VmErrorKind::TypeError
    );
    assert_eq!(
        eval(
            "let caught = false; try { String.prototype.split.call(null, ','); } catch (e) { caught = true; } caught"
        ),
        ValueView::Bool(true)
    );
}

/// Synthetic fixtures, not light-cljs output.
#[test]
fn string_split_coercion_and_boundaries() {
    for src in [
        "let a = '\\u{10000}'.split(''); a.length === 2 && a[0] === '\\ud800' && a[1] === '\\udc00'",
        "'abababa'.split('aba').join('|') === '|b|' && 'aaaaa'.split('aa').join('|') === '||a'",
        "'a,b,c'.split(',', 2.9).join('|') === 'a|b' && 'a,b'.split(',', 4294967297).join('|') === 'a'",
        "'a,b'.split(',', NaN).length === 0 && 'a,b'.split(',', Infinity).length === 0 && 'a,b'.split(',', null).length === 0",
        "'a,b'.split(',', -4294967296).length === 0 && 'a,b'.split(',', -1.9).length === 2",
        "'undefinedxundefined'.split(undefined).join('|') === 'undefinedxundefined'",
        "'undefinedxundefined'.split({toString() { return undefined; }}).join('|') === '|x|'",
        "'a,b'.split(',', {valueOf() { return undefined; }}).length === 0",
        "String.prototype.split.call(121, 2).join('|') === '1|1' && String.prototype.split.call(true, 'r').join('|') === 't|ue'",
        "let s = new String('a,b'); s.toString = () => 'c,d'; s.split(',').join('|') === 'c|d'",
        "let log = ''; let r = {toString() { log += 'r'; return 'a,b'; }}; let l = {valueOf() { log += 'l'; return 0; }}; let s = {toString() { log += 's'; return ','; }}; String.prototype.split.call(r, s, l).length === 0 && log === 'rls'",
        "let log = ''; let r = {get toString() { log += 'g'; return () => {log += 'r'; return {}; }; }, valueOf() { log += 'v'; return 'a,b'; }}; String.prototype.split.call(r, ',').join('|') === 'a|b' && log === 'grv'",
        "let hit = 0; let o = {valueOf() { hit++; return 1; }, toString() { hit++; return ','; }}; let caught = false; try { String.prototype.split.call(undefined, o, o); } catch(e) { caught = true; } caught && hit === 0",
        "let caught = false; try { 'a,b'.split({toString() { throw 42; }}, 0); } catch(e) { caught = e === 42; } caught",
        "let hit = 0; let caught = false; try { 'a,b'.split({toString() { hit++; return ','; }}, {valueOf() { throw 43; }}); } catch(e) { caught = e === 43; } caught && hit === 0",
        "let hit = 0; let caught = false; try { String.prototype.split.call({toString() { throw 44; }}, ',', {valueOf() { hit++; return 1; }}); } catch(e) { caught = e === 44; } caught && hit === 0",
        "let a = 'a,b'.split(','); let b = 'a,b'.split(','); a[0] = 'c'; a !== b && b[0] === 'a' && Object.getPrototypeOf(a) === Array.prototype && Object.keys(a).join('|') === '0|1'",
        "let a = ''.split(''); let b = ''.split(''); a !== b && a instanceof Array && a.length === 0",
        "let a = 'a,b'.split(','); let d = Object.getOwnPropertyDescriptor(a, '0'); d.value === 'a' && d.writable && d.enumerable && d.configurable",
        "let d = Object.getOwnPropertyDescriptor(String.prototype, 'split'); d.writable && !d.enumerable && d.configurable && d.value.name === 'split' && d.value.length === 2",
        "__rt.reduce(String.prototype.split.bind('a,b'), ',', __rt.vector(1)).join('|') === 'a'",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
    for src in [
        "new String.prototype.split()",
        "String.prototype.split.call(Object.create(null), ',')",
        "'a,b'.split(Object.create(null))",
        "'a,b'.split(',', Object.create(null))",
    ] {
        assert_eq!(failure(src).kind(), VmErrorKind::TypeError, "{src}");
    }
}

/// Synthetic fixtures, not light-cljs output. Yield/collect on migrated
/// tasks with an otherwise unreachable result array and partially copied text.
#[test]
fn string_split_long_work_yields_collects_and_migrates() {
    for (expression, expected_len, expected_first, expected_last, minimum_yields) in [
        (
            format!("'{}b'.split('aaaab')", "a".repeat(8192)),
            2,
            8188,
            0,
            32,
        ),
        (
            format!("'{}b'.split('{}b')", "a".repeat(8192), "a".repeat(8192)),
            2,
            0,
            0,
            32,
        ),
        (
            format!("'x{}z'.split('x')", "a".repeat(8192)),
            2,
            0,
            8193,
            32,
        ),
        (
            format!("'{}'.split('')", "a".repeat(1024)),
            1024,
            1,
            1,
            1024,
        ),
        (
            format!("'{}'.split('aa')", "a".repeat(2048)),
            1025,
            0,
            0,
            1025,
        ),
        (format!("'{}'.split('a', 1)", "a".repeat(8192)), 1, 0, 0, 0),
    ] {
        let src = format!(
            "let a = {expression}; a.length === {expected_len} && a[0].length === {expected_first} && a[a.length - 1].length === {expected_last}"
        );
        let rt = Runtime::new();
        let mut task = rt.spawn(&exe(&src), &[]).unwrap();
        let mut yields = 0;
        let mut complete = false;
        for _ in 0..5000 {
            let (next, outcome) = std::thread::spawn(move || {
                let outcome = task
                    .run_with(Budget::instructions(1), &mut NoEffects)
                    .unwrap();
                (task, outcome)
            })
            .join()
            .unwrap();
            task = next;
            match outcome {
                RunOutcome::Yielded => {
                    yields += 1;
                    rt.collect().unwrap();
                }
                RunOutcome::Complete(value) => {
                    assert_eq!(value.view(), ValueView::Bool(true), "{expression}");
                    complete = true;
                    break;
                }
                other => panic!("{other:?}"),
            }
        }
        assert!(
            complete && yields > minimum_yields,
            "complete={complete}, yields={yields}"
        );
        if expected_len == 1 {
            assert!(yields < 100, "small limit did not stop scanning");
        }
    }
}

/// Synthetic fixtures, not light-cljs output.
#[test]
fn string_search_coercion_and_boundaries() {
    for src in [
        "'abcabc'.indexOf('abc', 1.9) === 3 && 'abcabc'.lastIndexOf('abc', 2.9) === 0",
        "'abcabc'.lastIndexOf('abc', 3.9) === 3 && 'aaa'.lastIndexOf('aa', 1) === 1",
        "'abc'.indexOf('abcd') === -1 && 'abc'.lastIndexOf('abcd') === -1",
        "''.indexOf('') === 0 && ''.lastIndexOf('') === 0 && ''.lastIndexOf('x') === -1",
        "'abc'.indexOf('', -0.9) === 0 && 'abc'.lastIndexOf('', -0.9) === 0",
        "'abc'.indexOf('b', '1.9') === 1 && 'abc'.lastIndexOf('', undefined) === 3",
        "'abc'.lastIndexOf('b', null) === -1 && 'abc'.indexOf('a', false) === 0",
        "'undefined'.indexOf() === 0 && 'undefined'.lastIndexOf() === 0",
        "'a\\u{10000}b'.lastIndexOf('b') === 3 && '\\ud800x\\ud800'.indexOf('\\ud800', 1) === 2 && '\\ud800x\\ud800'.lastIndexOf('\\ud800', 1) === 0",
        "'\\ud800\\udc00'.indexOf('\\udc00') === 1 && '\\ud800\\udc00'.lastIndexOf('\\ud800') === 0",
        "String.prototype.indexOf.call(123, 2) === 1 && String.prototype.lastIndexOf.call(false, 'a') === 1",
        "new String('abcabc').indexOf(new String('abc'), new Number(1)) === 3 && new String('abcabc').lastIndexOf(new String('abc')) === 3",
        "let s = new String('abc'); s.toString = () => 'xyz'; s.indexOf('y') === 1",
        "let r = {toString: () => 'abc'}; String.prototype.indexOf.call(r, {toString: () => 'b'}, {valueOf: () => 1}) === 1",
        "let order = ''; let r = {toString: () => {order += 'r'; return 'abc'; }}; let s = {toString: () => {order += 's'; return 'b'; }, valueOf: () => {throw 1; }}; let p = {valueOf: () => {order += 'p'; return NaN; }, toString: () => {throw 2; }}; String.prototype.lastIndexOf.call(r, s, p) === 1 && order === 'rsp'",
        "let n = 0; let bad = {toString: () => {n++; throw 42; }}; let p = {valueOf: () => {n += 10; return 0; }}; let caught = 0; try { 'abc'.indexOf(bad, p); } catch (e) { caught = e; } caught === 42 && n === 1",
        "let n = 0; let s = {toString: () => {n++; return 'x'; }}; let caught = false; try { String.prototype.lastIndexOf.call(undefined, s); } catch (e) { caught = true; } caught && n === 0",
        "let bad = {toString: () => ({}), valueOf: () => ({})}; let caught = false; try { 'x'.lastIndexOf(bad); } catch (e) { caught = true; } caught",
        "let s = {toString: String.prototype.indexOf.bind('abc', {toString: () => 'b'})}; '1'.indexOf(s) === 0",
        "let f = String.prototype.indexOf.bind('abcabc', {toString: () => 'abc'}); f.apply(null, [{valueOf: () => 1}]) === 3",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
    for name in ["indexOf", "lastIndexOf"] {
        let src = format!(
            "let d = Object.getOwnPropertyDescriptor(String.prototype, '{name}'); \
             d.writable && !d.enumerable && d.configurable && \
             d.value.name === '{name}' && d.value.length === 1"
        );
        assert_eq!(eval(&src), ValueView::Bool(true), "{src}");
    }
}

/// Synthetic fixtures, not light-cljs output. Repetitive input exercises
/// prefix fallback, overlapping matches and scan work across budget slices.
#[test]
fn string_search_long_scans_yield_and_migrate() {
    for name in ["indexOf", "lastIndexOf"] {
        let src = format!("'{}b'.{name}('{}b')", "a".repeat(8192), "a".repeat(4096));
        let rt = Runtime::new();
        let mut task = rt.spawn(&exe(&src), &[]).unwrap();
        let mut yields = 0;
        let mut complete = false;
        for _ in 0..200 {
            let (next, outcome) = std::thread::spawn(move || {
                let outcome = task
                    .run_with(Budget::instructions(1), &mut NoEffects)
                    .unwrap();
                (task, outcome)
            })
            .join()
            .unwrap();
            task = next;
            match outcome {
                RunOutcome::Yielded => {
                    yields += 1;
                    rt.collect().unwrap();
                }
                RunOutcome::Complete(result) => {
                    assert_eq!(result.view(), ValueView::Number(4096.0), "{name}");
                    complete = true;
                    break;
                }
                other => panic!("{name}: {other:?}"),
            }
        }
        assert!(complete, "{name} exceeded instruction bound");
        assert!(
            yields > 32,
            "{name} did not budget prefix construction and scanning"
        );
    }
}

/// Synthetic fixtures, not light-cljs output. Includes the substring/concat/
/// empty-filler padding cases from QuickJS test_string; MIT notice above.
#[test]
fn string_substring_slice_concat_pad() {
    for src in [
        "'abcd'.substring(1, 3) === 'bc'",
        "'a'.concat('b', 'c') === 'abc'",
        "'abc'.padStart(Infinity, '') === 'abc'",
        "'abcd'.substring(3, 1) === 'bc'",
        "'abcd'.substring(-5, 2) === 'ab'",
        "'abcd'.substring(1) === 'bcd'",
        "'abcd'.slice(-1) === 'd'",
        "'abcd'.slice(0, -1) === 'abc'",
        "'abcd'.slice(2, 1) === ''",
        "'x@y'.slice(0, 1) + 'x@y'.slice(2) === 'xy'",
        "'a'.concat(1, null, undefined) === 'a1nullundefined'",
        "'abc'.padStart(6, '12') === '121abc'",
        "'abc'.padEnd(5) === 'abc  '",
        "'abc'.padStart(2, 'x') === 'abc'",
        "let caught = false; try { 'abc'.padStart(Infinity, 'x'); } catch (e) { caught = true; } caught",
        "'\\u{10000}'.slice(0, 1) === '\\ud800'",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
}

/// Synthetic fixtures, not light-cljs output.
#[test]
fn string_extract_combine_coercion_and_boundaries() {
    for src in [
        "'abcd'.substring(Infinity, -Infinity) === 'abcd' && 'abcd'.slice(-Infinity, Infinity) === 'abcd'",
        "'abcd'.substring(NaN, 2.9) === 'ab' && 'abcd'.slice(-2.9) === 'cd'",
        "'abcd'.substring(1, undefined) === 'bcd' && 'abcd'.slice(1, undefined) === 'bcd'",
        "'abcd'.substring(1, {valueOf: () => undefined}) === 'a' && 'abcd'.slice(1, {valueOf: () => undefined}) === ''",
        "'abc'.padStart(5, undefined) === '  abc' && 'abc'.padEnd(5, {toString: () => undefined}) === 'abcun'",
        "'abc'.padEnd(Infinity, '') === 'abc' && 'abc'.padStart(NaN) === 'abc' && 'abc'.padEnd(-Infinity) === 'abc'",
        "'abc'.padStart(5.9, '12') === '12abc' && 'abc'.padEnd('6.9', '12') === 'abc121'",
        "'a'.concat() === 'a' && ''.concat('', '') === '' && ''.padStart(0, '') === ''",
        "'abc'.slice(99) === '' && 'abc'.substring(99) === '' && 'abc'.slice(2, null) === ''",
        "'a'.padStart(2, '\\u{10000}') === '\\ud800a' && 'a'.padEnd(2, '\\udc00') === 'a\\udc00'",
        "String.prototype.substring.call(123, 1) === '23' && String.prototype.slice.call(false, -2) === 'se'",
        "String.prototype.concat.call(true, 1) === 'true1' && String.prototype.padStart.call(1, 2, 0) === '01'",
        "String.prototype.substring(0) === '' && new String('abc').slice(1) === 'bc'",
        "let s = new String('abc'); s.toString = () => 'xyz'; s.substring(1) === 'yz' && s.padEnd(5, '!') === 'xyz!!'",
        "let order = ''; let r = {toString: () => {order += 'r'; return 'abc'; }}; let a = {valueOf: () => {order += 'a'; return 1; }}; let b = {valueOf: () => {order += 'b'; return 2; }}; String.prototype.substring.call(r, a, b) === 'b' && order === 'rab'",
        "let order = ''; let r = {toString: () => {order += 'r'; return 'a'; }}; let a = {toString: () => {order += 'a'; return 1; }, valueOf: () => {throw 1; }}; String.prototype.concat.call(r, a, a) === 'a11' && order === 'raa'",
        "let n = 0; let f = {toString: () => {n++; throw 1; }}; 'abc'.padStart(2, f) === 'abc' && 'abc'.padEnd(NaN, f) === 'abc' && n === 0",
        "let n = 0; let f = {toString: () => {n++; return ''; }}; 'abc'.padEnd(Infinity, f) === 'abc' && n === 1",
        "let order = ''; let f = {toString: () => {order += 'f'; return 'x'; }}; let caught = false; try { 'abc'.padStart(Infinity, f); } catch(e) { caught = true; } caught && order === 'f'",
        "let n = 0; let bad = {valueOf: () => {throw 42; }}; let end = {valueOf: () => {n++; return 2; }}; let caught = 0; try { 'abc'.slice(bad, end); } catch(e) { caught = e; } caught === 42 && n === 0",
        "let n = 0; let bad = {toString: () => {throw 42; }}; let later = {toString: () => {n++; return 'x'; }}; let caught = 0; try { 'abc'.concat(bad, later); } catch(e) { caught = e; } caught === 42 && n === 0",
        "let f = String.prototype.concat.bind({toString: () => 'a'}, {toString: () => 'b'}); f.apply(null, ['c']) === 'abc'",
        "let s = {toString: String.prototype.concat.bind('a', {toString: () => 'b'})}; String.prototype.slice.call(s, 1) === 'b'",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
    for (name, length) in [
        ("substring", 2),
        ("slice", 2),
        ("concat", 1),
        ("padStart", 1),
        ("padEnd", 1),
    ] {
        let src = format!(
            "let d = Object.getOwnPropertyDescriptor(String.prototype, '{name}'); d.writable && !d.enumerable && d.configurable && d.value.name === '{name}' && d.value.length === {length}"
        );
        assert_eq!(eval(&src), ValueView::Bool(true), "{src}");
        for receiver in ["null", "undefined"] {
            let src = format!(
                "let n = 0; let a = {{toString: () => {{n++; return 'x'; }}, valueOf: () => {{n++; return 1; }}}}; let caught = false; try {{ String.prototype.{name}.call({receiver}, a, a); }} catch(e) {{caught = true;}} caught && n === 0"
            );
            assert_eq!(eval(&src), ValueView::Bool(true), "{src}");
        }
    }
    for name in ["padStart", "padEnd"] {
        for target in ["Infinity", "1073741824", "9007199254740991"] {
            let src = format!("'abc'.{name}({target}, 'x')");
            assert_eq!(failure(&src).kind(), VmErrorKind::RangeError, "{src}");
        }
    }
}

/// Synthetic fixtures, not light-cljs output. Construction of long results
/// must yield while retaining the source and partially constructed output.
#[test]
fn string_extract_combine_long_results_yield_collect_and_migrate() {
    for (i, expression) in [
        format!("'{}z'.substring(1)", "a".repeat(8192)),
        format!("'{}z'.slice(1)", "a".repeat(8192)),
        format!("'{}'.concat('{}z')", "a".repeat(4096), "a".repeat(4095)),
        "'z'.padStart(8192, 'a')".to_string(),
        "''.padEnd(8192, 'a').slice(0, -1).concat('z')".to_string(),
        // 601 whole parts cross the native assembly boundary twice.
        format!("''.concat({}'{}z')", "'a',".repeat(599), "a".repeat(7592)),
    ]
    .into_iter()
    .enumerate()
    {
        let rt = Runtime::new();
        let mut task = rt.spawn(&exe(&expression), &[]).unwrap();
        let mut yields = 0;
        let mut complete = false;
        // The new many-argument fixture also budgets its 600 operand pushes
        // and conversions. Keep the existing fixtures' bounds unchanged.
        for _ in 0..if i == 5 { 4000 } else { 400 } {
            let (next, outcome) = std::thread::spawn(move || {
                let outcome = task
                    .run_with(Budget::instructions(1), &mut NoEffects)
                    .unwrap();
                (task, outcome)
            })
            .join()
            .unwrap();
            task = next;
            match outcome {
                RunOutcome::Yielded => {
                    yields += 1;
                    rt.collect().unwrap();
                }
                RunOutcome::Complete(result) => {
                    assert_eq!(
                        result.string().unwrap().to_string(),
                        format!("{}z", "a".repeat(8191))
                    );
                    complete = true;
                    break;
                }
                other => panic!("{other:?}"),
            }
        }
        assert!(complete, "construction exceeded instruction bound");
        if i == 2 {
            // Concat now links two immutable operands without copying 8192
            // units. Its conversion/VM steps still yield and migrate.
            assert!(yields > 0, "concat did not yield");
        } else {
            assert!(yields > 32, "construction did not budget copying");
        }
    }
}

/// A host for budgeted runs of code that performs no host effects.
struct NoEffects;

/// Synthetic fixtures, not light-cljs output. The first object adapts
/// QuickJS tests/test_builtin.js test_json (7d1c13c5d8f8); the copyright
/// and MIT notice retained at string_code_unit_access applies here too.
#[test]
fn json_parse() {
    for src in [
        r#"let a = JSON.parse('{"x":1,"y":true,"z":null,"a":[1,2,3],"s":"str"}'); a.x === 1 && a.y === true && a.z === null && a.a[2] === 3 && a.a.length === 3 && a.s === 'str'"#,
        r#"JSON.parse('1') === 1 && Object.is(JSON.parse(' -0 '), -0) && JSON.parse('1e400') === Infinity"#,
        r#"JSON.parse('[1, [2, {}]]')[1][0] === 2 && JSON.parse('"\\u20ac\\n"') === '€\n' && JSON.parse('"\\ud800"').length === 1"#,
        r#"JSON.parse('"\\ud800"').charCodeAt(0) === 0xd800 && JSON.parse('"\\udc00"').charCodeAt(0) === 0xdc00 && JSON.parse('"\\ud83d\\ude00"') === '😀'"#,
        r#"JSON.parse('"\\\"\\\\\\/\\b\\f\\n\\r\\t"') === '\"\\/\b\f\n\r\t'"#,
        r#"let a = JSON.parse('{"a":1,"b":3,"a":2}'); a.a === 2 && Object.keys(a).join(',') === 'a,b'"#,
        r#"Object.keys(JSON.parse('{"b":1,"a":2,"1":3}')).join(',') === '1,b,a'"#,
        r#"let a = JSON.parse('{"__proto__":1}'); Object.getPrototypeOf(a) === Object.prototype && a.__proto__ === 1 && Object.getOwnPropertyDescriptor(a, '__proto__').enumerable"#,
        r#"JSON.parse(null) === null && JSON.parse(true) === true && JSON.parse(42) === 42 && JSON.parse({toString() { return '[4]'; }})[0] === 4"#,
        r#"typeof JSON === 'object' && Object.getPrototypeOf(JSON) === Object.prototype && JSON.parse.length === 2 && JSON.parse.name === 'parse' && Object.keys(JSON).length === 0"#,
        r#"let d = Object.getOwnPropertyDescriptor(JSON, 'parse'); d.writable && !d.enumerable && d.configurable"#,
        r#"JSON.parse(' \t\r\nfalse\r\n') === false && JSON.parse('[]').length === 0 && Object.keys(JSON.parse('{}')).length === 0"#,
        r#"JSON.parse('9007199254740993') === 9007199254740992 && JSON.parse('2.4703282292062327e-324') === 0 && JSON.parse('2.4703282292062328e-324') === Number.MIN_VALUE"#,
        r#"JSON.parse('1', {}) === 1 && JSON.parse('1', null) === 1"#,
        r#"JSON.parse.call(null, '[3]')[0] === 3 && JSON.parse.apply(null, ['4']) === 4 && JSON.parse.bind(null, '5')() === 5"#,
        r#"let log = ''; let a = JSON.parse({toString() { log += 's'; return {}; }, valueOf() { log += 'v'; return '6'; }}); a === 6 && log === 'sv'"#,
        r#"let caught = false; try { JSON.parse({toString() { throw 42; }}); } catch (e) { caught = e === 42; } caught"#,
        r#"JSON.parse('"\ud800"').charCodeAt(0) === 0xd800"#,
        r#"let JSON = 7; JSON === 7"#,
        r#"JSON.parse('1', Symbol('reviver')) === 1"#,
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
    for text in [
        "",
        "{'a':1}",
        "[1,]",
        "01",
        "+1",
        ".5",
        "5.",
        "NaN",
        "Infinity",
        "0x1",
        "-01",
        "1e",
        "1e+",
        "--1",
        "[",
        "{",
        "[1",
        "{\"a\":",
        "\"\\x41\"",
        "\"a\u{0001}\"",
        "1 2",
        "{\"a\" 1}",
        "\n\"  \\x\"",
        "\n{ \"a\": x }",
        "\u{a0}1",
        "[true false]",
        "{\"a\":1,}",
        "\"\\u123\"",
        "\"\\uZZZZ\"",
        "\"unterminated",
        "truex",
        "[,,]",
    ] {
        let code = compile_function("function parse(text) { try { JSON.parse(text); } catch (e) { return true; } return false; }").unwrap();
        match run(&code, &[Primitive::String(text.into())]) {
            RunOutcome::Complete(v) => assert_eq!(v.view(), ValueView::Bool(true), "{text:?}"),
            other => panic!("{text:?}: {other:?}"),
        }
        let uncaught =
            compile_function("function parse(text) { return JSON.parse(text); }").unwrap();
        match run(&uncaught, &[Primitive::String(text.into())]) {
            RunOutcome::Failed(e) => assert_eq!(e.kind(), VmErrorKind::SyntaxError, "{text:?}"),
            other => panic!("{text:?}: {other:?}"),
        }
    }
    assert_eq!(
        failure("JSON.parse('1', function() {})").kind(),
        VmErrorKind::TypeError
    );
    assert_eq!(
        failure("new JSON.parse('1')").kind(),
        VmErrorKind::TypeError
    );
    assert_eq!(failure("JSON()").kind(), VmErrorKind::TypeError);
    assert_eq!(failure("new JSON()").kind(), VmErrorKind::TypeError);
    assert_eq!(
        failure("JSON.parse(Symbol('1'))").kind(),
        VmErrorKind::TypeError
    );
    assert_eq!(
        failure("JSON.parse({toString() { return Symbol('1'); }})").kind(),
        VmErrorKind::TypeError
    );
    assert_eq!(failure("JSON.parse()").kind(), VmErrorKind::SyntaxError);
    let code = compile_function("function parse(text) { try { JSON.parse(text); } catch (e) { return true; } return false; }").unwrap();
    assert!(
        matches!(run(&code, &[Primitive::String("[".repeat(100_000).as_str().into())]), RunOutcome::Complete(v) if v.view() == ValueView::Bool(true))
    );
}

/// Synthetic fixture, not light-cljs output. Both method lookup and the
/// ToString call suspend; parsing then retains its own container roots.
#[test]
fn json_parse_coercion_waits_collects_and_migrates() {
    let rt = Runtime::new();
    let src = r#"
        let log = '';
        let result = JSON.parse({get toString() {
            log += 'g'; __rt.need(1, 1);
            return function() { log += 's'; __rt.need(2, 1); return '{"a":[{"n":7}]}'; };
        }});
        log === 'gs' && result.a[0].n === 7
    "#;
    let mut task = rt.spawn(&exe(src), &[]).unwrap();
    let mut waited = 0;
    for _ in 0..1000 {
        let (next, outcome) = std::thread::spawn(move || {
            let outcome = task
                .run_with(Budget::instructions(1), &mut NoEffects)
                .unwrap();
            (task, outcome)
        })
        .join()
        .unwrap();
        task = next;
        match outcome {
            RunOutcome::Yielded => {
                rt.collect().unwrap();
            }
            RunOutcome::Waiting(_) => {
                waited += 1;
                assert!(waited <= 2);
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
            RunOutcome::Complete(value) => {
                assert_eq!(value.view(), ValueView::Bool(true));
                assert_eq!(waited, 2);
                return;
            }
            other => panic!("{other:?}"),
        }
    }
    panic!("JSON.parse did not finish");
}

/// Synthetic fixture, not light-cljs output. Budget and collect throughout
/// whitespace, escape decoding, long numeric conversion and object creation.
#[test]
fn json_parse_large_input_yields_and_traces_partial_results() {
    let rt = Runtime::new();
    let text = format!(
        "{}[{{\"keep\":[{{\"n\":7}}]}},\"{}\",1{}e-12000,[{}]]{}",
        " ".repeat(16384),
        "\\ud800".repeat(4096),
        "0".repeat(12000),
        std::iter::repeat_n("{\"v\":8}", 512)
            .collect::<Vec<_>>()
            .join(","),
        "\t".repeat(16384)
    );
    let code = compile_function("function parse(text) { let a = JSON.parse(text); return a[0].keep[0].n === 7 && a[1].length === 4096 && a[1].charCodeAt(4000) === 0xd800 && a[2] === 1 && a[3].length === 512 && a[3][511].v === 8; }").unwrap();
    let root = rt.root(Primitive::String(text.as_str().into()));
    let task = rt.spawn(&code, &[&root]).unwrap();
    drop(root);
    let mut yields = 0;
    for _ in 0..10000 {
        match task
            .run_with(Budget::instructions(1), &mut NoEffects)
            .unwrap()
        {
            RunOutcome::Yielded => {
                yields += 1;
                rt.collect().unwrap();
            }
            RunOutcome::Complete(value) => {
                assert_eq!(value.view(), ValueView::Bool(true));
                assert!(yields > 500, "large input must consume budget");
                return;
            }
            other => panic!("{other:?}"),
        }
    }
    panic!("JSON.parse did not finish");
}

#[test]
fn json_parse_depth_and_error_positions() {
    let code = compile_function("function parse(text) { let a = JSON.parse(text); for (let i = 0; i < 512; i++) { a = a[0]; } return a === 7; }").unwrap();
    let text = format!("{}7{}", "[".repeat(512), "]".repeat(512));
    assert!(
        matches!(run(&code, &[Primitive::String(text.as_str().into())]), RunOutcome::Complete(v) if v.view() == ValueView::Bool(true))
    );
    let code = compile_function("function parse(text) { return JSON.parse(text); }").unwrap();
    let deep = format!("{}7{}", "[".repeat(513), "]".repeat(513));
    match run(&code, &[Primitive::String(deep.as_str().into())]) {
        RunOutcome::Failed(e) => {
            assert_eq!(e.kind(), VmErrorKind::SyntaxError);
            assert!(e.message().contains("nesting limit of 512 exceeded"));
        }
        other => panic!("{other:?}"),
    }
    let caught = compile_function("function parse(text) { try { JSON.parse(text); } catch (e) { return true; } return false; }").unwrap();
    assert!(
        matches!(run(&caught, &[Primitive::String(deep.as_str().into())]), RunOutcome::Complete(v) if v.view() == ValueView::Bool(true))
    );
    for (text, position) in [
        ("\r\n{\"a\":x}", "line 2 column 6 (UTF-16 offset 7)"),
        ("[\"😀\",x]", "line 1 column 7 (UTF-16 offset 6)"),
        ("\n\"  \\x\"", "line 2 column 5 (UTF-16 offset 5)"),
    ] {
        match run(&code, &[Primitive::String(text.into())]) {
            RunOutcome::Failed(e) => {
                assert_eq!(e.kind(), VmErrorKind::SyntaxError);
                assert!(e.message().contains(position), "{}", e.message());
            }
            other => panic!("{other:?}"),
        }
    }
}

/// Synthetic fixtures, not light-cljs output. Isolate each token category:
/// another token's continuation boundaries cannot satisfy its yield count.
#[test]
fn json_parse_token_scans_are_budgeted() {
    let units = 65536;
    let code = compile_function("function parse(text) { let a = JSON.parse(text); return typeof a === 'string' ? a.length : a; }").unwrap();
    for (name, text, expected, minimum) in [
        (
            "whitespace",
            format!("{}true", " ".repeat(units)),
            ValueView::Bool(true),
            units / 256,
        ),
        (
            "raw string",
            format!("\"{}\"", "x".repeat(units)),
            ValueView::Number(units as f64),
            units * 2 / 256,
        ),
        (
            "escaped string",
            format!("\"{}\"", "\\ud800".repeat(units)),
            ValueView::Number(units as f64),
            units * 2 / 256,
        ),
        (
            "number",
            format!("1{}e-{units}", "0".repeat(units)),
            ValueView::Number(1.0),
            units * 2 / 256,
        ),
    ] {
        let rt = Runtime::new();
        let root = rt.root(Primitive::String(text.as_str().into()));
        let task = rt.spawn(&code, &[&root]).unwrap();
        let mut yields = 0;
        let mut complete = false;
        for _ in 0..4096 {
            match task
                .run_with(Budget::instructions(1), &mut NoEffects)
                .unwrap()
            {
                RunOutcome::Yielded => yields += 1,
                RunOutcome::Complete(value) => {
                    assert_eq!(value.view(), expected, "{name}");
                    assert!(
                        yields >= minimum,
                        "{name}: {yields} yields, expected at least {minimum}"
                    );
                    complete = true;
                    break;
                }
                other => panic!("{name}: {other:?}"),
            }
        }
        assert!(complete, "{name} did not finish");
    }
}

/// Synthetic fixtures, not light-cljs output. The limit counts decoded
/// UTF-16 units, so different spellings of a boundary key still duplicate.
#[test]
fn json_parse_member_name_limit() {
    let key = "x".repeat(4096);
    let escaped = "\\u0078".repeat(4096);
    let text = format!("{{\"{key}\":1,\"{escaped}\":2}}");
    let code = compile_function("function parse(text) { let a = JSON.parse(text); let keys = Object.keys(a); return keys.length === 1 && keys[0].length === 4096 && a[keys[0]] === 2; }").unwrap();
    assert!(
        matches!(run(&code, &[Primitive::String(text.as_str().into())]), RunOutcome::Complete(v) if v.view() == ValueView::Bool(true))
    );
    let code = compile_function("function parse(text) { return JSON.parse(text); }").unwrap();
    let caught = compile_function("function parse(text) { try { JSON.parse(text); } catch (e) { return true; } return false; }").unwrap();
    for key in ["x".repeat(4097), "\\u0078".repeat(4097)] {
        let text = format!("{{\"{key}\":1}}");
        match run(&code, &[Primitive::String(text.as_str().into())]) {
            RunOutcome::Failed(e) => {
                assert_eq!(e.kind(), VmErrorKind::SyntaxError);
                assert!(
                    e.message()
                        .contains("member name limit of 4096 UTF-16 units exceeded")
                );
            }
            other => panic!("{other:?}"),
        }
        assert!(
            matches!(run(&caught, &[Primitive::String(text.as_str().into())]), RunOutcome::Complete(v) if v.view() == ValueView::Bool(true))
        );
    }
}

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
            | Opcode::Method
            | Opcode::Call
            | Opcode::GetCapture
            | Opcode::GetField
            | Opcode::PutField
            | Opcode::DefineField
            | Opcode::GetBuiltinGlobal
            | Opcode::CallMethod
            | Opcode::GetGlobal
            | Opcode::GetGlobalOrUndefined
            | Opcode::PutGlobal
            | Opcode::DeclareGlobalVar
            | Opcode::DeclareGlobalFunction
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
        Opcode::CompareLt,
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
    // A reference to the global `eval` (clj-quickjs-106) stays unsupported.
    {
        let (src, column) = ("x = eval;", 5);
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
    let (kind, message, line, column) = failure_at("const a = 0;\n[(a)] = [1];");
    assert_eq!((kind, line, column), (VmErrorKind::TypeError, 2, 3));
    assert_eq!(message, "assignment to constant variable 'a'");
    let (kind, message, line, column) = failure_at("\n[(a)] = [1]; let a;");
    assert_eq!((kind, line, column), (VmErrorKind::ReferenceError, 2, 3));
    assert!(message.contains("'a'"), "{message}");
    assert!(!message.contains("(a)"), "{message}");
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
    use SyntaxErrorKind::Invalid;
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

    assert_eq!(num("let o = 1;\nObject = o;"), 1.0);
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

/// `test_number` rows 365-380 from research/quickjs/tests/test_builtin.js,
/// revision 7d1c13c5d8f8. The full QuickJS copyright and MIT permission notice
/// is retained on string_code_unit_access above. Remaining cases are synthetic
/// fixtures, not light-cljs output.
#[test]
fn parse_int_and_parse_float() {
    for (src, expected) in [
        (r#"parseInt("123")"#, 123.0),
        (r#"parseInt("  123r")"#, 123.0),
        (r#"parseInt("0x123")"#, 0x123 as f64),
        (r#"parseInt("0o123")"#, 0.0),
        (r#"+"  123   ""#, 123.0),
        (r#"+"0b111""#, 7.0),
        (r#"+"0o123""#, 83.0),
        (r#"parseFloat("2147483647")"#, 2147483647.0),
        (r#"parseFloat("2147483648")"#, 2147483648.0),
        (r#"parseFloat("-2147483647")"#, -2147483647.0),
        (r#"parseFloat("-2147483648")"#, -2147483648.0),
        (r#"parseFloat("0x1234")"#, 0.0),
        (r#"parseFloat("Infinity")"#, f64::INFINITY),
        (r#"parseFloat("-Infinity")"#, f64::NEG_INFINITY),
        (r#"parseFloat("123.2")"#, 123.2),
        (r#"parseFloat("123.2e3")"#, 123200.0),
        (r#"parseInt("ff", 16)"#, 255.0),
        (r#"parseInt("0x10", 16)"#, 16.0),
        (r#"parseInt("0x10", 10)"#, 0.0),
        (r#"parseInt("0x10", 36)"#, 42804.0),
        (r#"parseInt("-0x10")"#, -16.0),
        (r#"parseInt("123", 0)"#, 123.0),
        (r#"parseInt("9007199254740993")"#, 9007199254740992.0),
        (r#"parseFloat(" .5e1x")"#, 5.0),
        (r#"parseFloat("Infinityx")"#, f64::INFINITY),
        (r#"parseFloat("+Infinityx")"#, f64::INFINITY),
        (r#"parseFloat("1e400")"#, f64::INFINITY),
        (r#"parseFloat("1e+")"#, 1.0),
        (r#"parseFloat("12.e-x")"#, 12.0),
        (r#"parseInt("11", 4294967298)"#, 3.0),
        (r#"parseInt("11", -4294967294)"#, 3.0),
        (r#"parseInt("10", 2.9)"#, 2.0),
        (r#"parseInt("0b11")"#, 0.0),
        (r#"parseInt("010")"#, 10.0),
        (r#"parseInt("10", Infinity)"#, 10.0),
        (r#"parseInt("10", null)"#, 10.0),
        (r#"parseInt("10", "16")"#, 16.0),
        (r#"parseInt("z!", 36)"#, 35.0),
        (r#"parseInt("9007199254740995")"#, 9007199254740996.0),
        (r#"parseInt("20000000000001", 16)"#, 9007199254740992.0),
        (r#"parseInt("20000000000003", 16)"#, 9007199254740996.0),
        (r#"parseInt(123.9)"#, 123.0),
        (r#"parseFloat(123.9)"#, 123.9),
        (r#"parseInt("\uFEFF\u00A0 -12x")"#, -12.0),
        (r#"parseFloat("\u2028\u3000 12.5x")"#, 12.5),
        (r#"parseFloat("1\uD800")"#, 1.0),
        (r#"parseInt("1\uD800")"#, 1.0),
    ] {
        assert_eq!(num(src), expected, "{src}");
    }
    for src in [
        r#"parseInt("12", 37)"#,
        r#"parseInt("12", 1)"#,
        r#"parseInt("")"#,
        r#"parseInt(" 0x")"#,
        r#"parseInt("+ 1")"#,
        r#"parseFloat("e3")"#,
        r#"parseFloat(".")"#,
        r#"parseFloat("infinity")"#,
        "parseInt()",
        "parseFloat()",
        "parseInt(null)",
        "parseFloat(true)",
    ] {
        assert!(num(src).is_nan(), "{src}");
    }
    for src in [
        r#"parseInt("  -0")"#,
        r#"parseInt("-0")"#,
        r#"parseFloat("-.0")"#,
    ] {
        assert_eq!(num(src).to_bits(), (-0.0f64).to_bits(), "{src}");
    }
    for src in [
        "1 / parseInt('-0') === -Infinity",
        "Number.parseInt === parseInt && Number.parseFloat === parseFloat",
        "parseInt.length === 2 && parseFloat.length === 1 && parseInt.name === 'parseInt' && parseFloat.name === 'parseFloat'",
        "let parseInt = () => 7; parseInt('123') === 7",
        "let o = {toString: () => 'ff', valueOf: () => 0}; parseInt(o, 16) === 255",
        "let order = ''; let s = {toString: () => {order += 's'; return '11';}}; let r = {valueOf: () => {order += 'r'; return 2;}}; parseInt(s, r) === 3 && order === 'sr'",
        "let n = 0; try {parseInt({toString: () => {throw 42;}}, {valueOf: () => {n++; return 10;}});} catch(e) {n += e;} n === 42",
        "let n = 0; parseFloat('1', {valueOf: () => {n++; return 2;}}) === 1 && n === 0",
        "let caught = false; try {new parseInt('1');} catch(e) {caught = true;} caught",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
    assert_eq!(failure("new parseInt('1')").kind(), VmErrorKind::TypeError);
    assert_eq!(
        failure("new parseFloat('1')").kind(),
        VmErrorKind::TypeError
    );
}

/// Synthetic fixtures, not light-cljs output. Both operands stay rooted
/// across callback waits and migration; their observable conversions run once.
#[test]
fn parse_numbers_coercion_waits_yields_collects_and_migrates() {
    for expression in [
        "parseInt(s, r) === 255 && order === 'sr'",
        "Number.parseInt.call(null, s, r) === 255 && order === 'sr'",
        "parseFloat({toString: () => {order += 'f'; __rt.need(1481, 1); return '12.5x';}}) === 12.5 && order === 'f'",
        "parseInt({toString: parseFloat.bind(null, {toString: () => {order += 'f'; __rt.need(1481, 1); return '12.5x';}})}) === 12 && order === 'f'",
        "(() => {let caught = 0; try {parseInt(s, {valueOf: () => {order += 'r'; __rt.need(1482, 1); throw 42;}});} catch(e) {caught = e;} return caught === 42 && order === 'sr';})()",
    ] {
        let rt = Runtime::new();
        let src = format!(
            "let order = ''; let s = {{toString: () => {{order += 's'; __rt.need(1481, 1); return 'ff';}}}}; \
             let r = {{valueOf: () => {{order += 'r'; __rt.need(1482, 1); return 16;}}}}; {expression}"
        );
        let mut task = rt.spawn(&exe(&src), &[]).unwrap();
        let mut waits = 0;
        let mut complete = false;
        for _ in 0..500 {
            let (next, outcome) = std::thread::spawn(move || {
                let outcome = task
                    .run_with(Budget::instructions(1), &mut NoEffects)
                    .unwrap();
                (task, outcome)
            })
            .join()
            .unwrap();
            task = next;
            match outcome {
                RunOutcome::Yielded => {
                    rt.collect().unwrap();
                }
                RunOutcome::Waiting(_) => {
                    waits += 1;
                    assert!(waits <= 2, "repeated callback: {expression}");
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
        assert!(complete, "instruction bound exceeded: {expression}");
        assert_eq!(
            waits,
            if expression.contains("order === 'sr'") {
                2
            } else {
                1
            }
        );
    }
}

/// Synthetic fixtures, not light-cljs output. Scans must yield even when
/// all operands are already primitives; retained decimal storage is bounded.
#[test]
fn parse_numbers_long_inputs_are_budgeted_and_rounded() {
    for (expression, expected) in [
        (format!("parseInt('{}1', 2)", "0".repeat(8192)), 1.0),
        (format!("parseInt('{}ff', 16)", " ".repeat(8192)), 255.0),
        (format!("parseFloat('0.{}1e8193')", "0".repeat(8192)), 1.0),
        (format!("parseFloat('1.{}1')", "0".repeat(8192)), 1.0),
    ] {
        let rt = Runtime::new();
        let task = rt.spawn(&exe(&expression), &[]).unwrap();
        let mut yields = 0;
        let mut complete = false;
        for _ in 0..100 {
            match task
                .run_with(Budget::instructions(1), &mut NoEffects)
                .unwrap()
            {
                RunOutcome::Yielded => {
                    yields += 1;
                    rt.collect().unwrap();
                }
                RunOutcome::Complete(result) => {
                    assert_eq!(result.view(), ValueView::Number(expected));
                    complete = true;
                    break;
                }
                other => panic!("{other:?}"),
            }
        }
        assert!(complete);
        assert!(yields >= 8192 / 256, "scan did not yield: {yields}");
    }
    for name in ["parseInt", "parseFloat"] {
        let src = format!(
            "let d = Object.getOwnPropertyDescriptor(Number, '{name}'); \
             d.value === {name} && d.writable && !d.enumerable && d.configurable && \
             Object.getPrototypeOf({name}) === Function.prototype"
        );
        assert_eq!(eval(&src), ValueView::Bool(true));
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
    // Unless both operands are static Numbers, preserve their primitive
    // types for the comparison (a dynamic operand may now be a BigInt).
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
        ops.contains(&Opcode::CompareLt) && !ops.contains(&Opcode::Lt),
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
    assert!(opcodes(&exe("let t = true; 2 ** t")).contains(&Opcode::ToNumberPair));
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

    // ToNumeric preserves BigInt; Inc/Dec apply a unit of the same type.
    let ops = opcodes(&exe("let a = 1; a++; --a"));
    for op in [
        Opcode::ToNumeric,
        Opcode::Inc,
        Opcode::Dec,
        Opcode::SetLocal,
    ] {
        assert!(ops.contains(&op), "{op:?} missing from {ops:?}");
    }
    // An update is numeric: no second ToNumeric before negation.
    let ops = opcodes(&exe("let a = 1; -a++"));
    assert_eq!(
        ops.iter().filter(|op| **op == Opcode::ToNumeric).count(),
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
        Opcode::ToNumeric,
        Opcode::PutElem,
    ] {
        assert!(ops.contains(&op), "{op:?} missing from {ops:?}");
    }
    let ops = opcodes(&exe("let o = {}; ++o.p;"));
    for op in [Opcode::Dup, Opcode::GetField, Opcode::Inc, Opcode::PutField] {
        assert!(ops.contains(&op), "{op:?} missing from {ops:?}");
    }
    // An update is numeric: no second ToNumeric before negation.
    let ops = opcodes(&exe("let o = {p: 1}; -o.p++"));
    assert_eq!(
        ops.iter().filter(|op| **op == Opcode::ToNumeric).count(),
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
    // A compound assignment may return BigInt, so `+` must preserve its
    // runtime type and check for mixed arithmetic.
    let ops = opcodes(&exe("let o = {p: 1}; 1 + (o.p *= 2)"));
    assert!(ops.contains(&Opcode::Plus), "{ops:?}");
    assert!(!ops.contains(&Opcode::Add), "{ops:?}");
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

/// Synthetic fixtures, not light-cljs output. The nested receiver row follows
/// QuickJS test_language.js test_optional_chaining; the copyright/license
/// notice on object_literals_and_named_properties covers this adapted row.
#[test]
fn object_literal_methods() {
    assert_eq!(num("({b() { return this._b; }, _b: {c: 42}}).b().c"), 42.0);
    assert_eq!(num("({get() { return 1; }}).get()"), 1.0);
    assert_eq!(num("({static() { return 42; }}).static()"), 42.0);
    assert_eq!(eval("({m() {}}).m.prototype"), ValueView::Undefined);
    assert_eq!(
        eval("let o = {m() { return this; }}; o.m() === o"),
        ValueView::Bool(true)
    );
    assert_eq!(string("({m() {}}).m.name"), "m");
    assert_eq!(string("({1.5() {}})[1.5].name"), "1.5");
    assert_eq!(string(r#"({"s"() {}}).s.name"#), "s");
    assert_eq!(num("({if() { return 42; }}).if()"), 42.0);
    assert_eq!(num("({async() { return 42; }}).async()"), 42.0);
    assert_eq!(
        num("({set(a = 42) { return arguments.length + a; }}).set()"),
        42.0
    );
    assert_eq!(
        eval("let o = {m() { return () => this; }}; o.m()() === o"),
        ValueView::Bool(true)
    );
    assert_eq!(
        eval("let f = ({m() { return this; }}).m; f()"),
        ValueView::Undefined
    );
    assert_eq!(
        eval(
            "let o = {m() {}}; let d = Object.getOwnPropertyDescriptor(o, 'm'); d.writable && d.enumerable && d.configurable"
        ),
        ValueView::Bool(true)
    );
    assert_eq!(
        failure("let o = {m() {}}; new o.m()").kind(),
        VmErrorKind::TypeError
    );
    assert_eq!(
        num("({__proto__: null, __proto__() { return 42; }}).__proto__()"),
        42.0
    );
    assert_eq!(
        num("const a = 20, b = 22; ({m() { return a + b; }}).m()"),
        42.0
    );
    assert_eq!(num("let m = 42; ({m() { return m; }}).m()"), 42.0);
    assert_eq!(
        num("let o = {x: 1, m() { return this.x; }}; o.m.call({x: 42})"),
        42.0
    );
    assert_eq!(
        num("let i = 0; let o = {m() { i = i + 1; return i; }}; o.m() + o.m()"),
        3.0
    );
    assert_eq!(num("({m(a, b = 1, c) {}}).m.length"), 1.0);
    assert_eq!(
        eval("Object.getOwnPropertyDescriptor(({m() {}}).m, 'prototype') === undefined"),
        ValueView::Bool(true)
    );
}

/// Synthetic fixtures, not light-cljs output: methods inherit strictness.
#[test]
fn object_literal_methods_in_sloppy_scripts() {
    let source = "let f = ({m() { return this; }}).m; f()";
    let exe = compile_script_with("sloppy-method", source, ScriptOptions::SLOPPY).unwrap();
    assert!(!exe.is_strict());
    let spec = exe.to_spec();
    assert_eq!(spec.functions.len(), 1);
    assert!(!spec.functions[0].is_strict());
    assert!(!spec.functions[0].to_spec().constructor);
    // Global-object substitution is explicitly unsupported (clj-quickjs-104).
    for source in [source, "({m() { return this; }}).m.call(undefined)"] {
        let exe = compile_script_with("sloppy-method", source, ScriptOptions::SLOPPY).unwrap();
        let RunOutcome::Failed(error) = run(&exe, &[]) else {
            panic!("sloppy this substitution must fail: {source}")
        };
        assert_eq!(error.kind(), VmErrorKind::UnsupportedOperand, "{source}");
        assert!(error.message().contains("sloppy-mode"), "{error}");
    }
    for source in [
        "({m() { return 010; }}).m() === 8",
        "({m(a = 010) { return a; }}).m() === 8",
        "let o = {m() { return this; }}; o.m() === o && o.m.call(o) === o",
        "({m() { 'use strict'; return this; }}).m.call(undefined) === undefined",
        "({m() { 'use strict'; return this; }}).m.call(42) === 42",
        "let o = {m() { 'use strict'; }, n(a = 010) { return a; }}; o.n() === 8",
    ] {
        let exe = compile_script_with("sloppy-method", source, ScriptOptions::SLOPPY).unwrap();
        match run(&exe, &[]) {
            RunOutcome::Complete(value) => {
                assert_eq!(value.view(), ValueView::Bool(true), "{source}")
            }
            other => panic!("{source}: expected Complete, got {other:?}"),
        }
    }
}

/// Synthetic fixtures, not light-cljs output: scripts, enclosing functions
/// and method-local directives all enforce strict-mode early errors.
#[test]
fn object_literal_methods_inherit_strictness() {
    for (source, options) in [
        ("({m() { return 010; }}).m()", ScriptOptions::STRICT),
        (
            "'use strict'; ({m() { return 010; }}).m()",
            ScriptOptions::SLOPPY,
        ),
        (
            "function f() { 'use strict'; return {m() { return 010; }}; }",
            ScriptOptions::SLOPPY,
        ),
        (
            "({m() { 'use strict'; return 010; }})",
            ScriptOptions::SLOPPY,
        ),
        ("({m(a = 010) {}})", ScriptOptions::STRICT),
    ] {
        let error = compile_script_with("strict-method", source, options).unwrap_err();
        assert_eq!(
            error.kind,
            SyntaxErrorKind::InvalidToken,
            "{source}: {error}"
        );
    }
    for (source, options) in [
        (
            "({m() { return this; }}).m.call(undefined)",
            ScriptOptions::STRICT,
        ),
        (
            "'use strict'; ({m() { return this; }}).m.call(undefined)",
            ScriptOptions::SLOPPY,
        ),
        (
            "function f() { 'use strict'; return {m() { return this; }}; } f().m.call(undefined)",
            ScriptOptions::SLOPPY,
        ),
    ] {
        let exe = compile_script_with("strict-method", source, options).unwrap();
        match run(&exe, &[]) {
            RunOutcome::Complete(value) => {
                assert_eq!(value.view(), ValueView::Undefined, "{source}")
            }
            other => panic!("{source}: expected Complete, got {other:?}"),
        }
    }
}

/// Corrupted bytecode must account for a method's extra home-object operand.
#[test]
fn method_bytecode_requires_a_home_object() {
    let mut spec = exe("(() => {})").to_spec();
    let at = decode(&spec.code)
        .into_iter()
        .find(|(_, op)| *op == Opcode::Function)
        .unwrap()
        .0;
    spec.code[at] = Opcode::Method as u8;
    assert_eq!(
        Executable::load(spec).unwrap_err().kind,
        ValidationErrorKind::StackUnderflow {
            opcode: Opcode::Method,
            depth: 0
        }
    );
    let spec = exe("let x = 1; ({m() { return x; }}).m()").to_spec();
    assert_eq!(Executable::load(spec.clone()).unwrap().to_spec(), spec);
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
    // Engine errors inherit through their NativeError and Error prototypes.
    let src = "let e; try { nope; } catch (x) { e = x; } e instanceof Object";
    assert_eq!(eval(src), ValueView::Bool(true), "{src}");
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
    // A Function.prototype name that does not exist yet remains unsupported.
    assert_eq!(
        failure("function f() {} 'toString' in f").kind(),
        VmErrorKind::UnsupportedOperand
    );
    assert_eq!(
        eval("let e; try { nope; } catch (x) { e = x; } 'message' in e"),
        ValueView::Bool(true)
    );
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
    {
        let (src, needle) = ("let b = true;\nb.x;", "of a boolean");
        let (kind, message, line, _) = failure_at(src);
        assert_eq!(kind, VmErrorKind::UnsupportedOperand, "{src}: {message}");
        assert!(message.contains(needle), "{src}: {message}");
        assert_eq!(line, 2, "{src}");
    }
    assert_eq!(
        eval(
            "let e; try { null.x; } catch (c) { e = c; } e.message === \"cannot read property 'x' of null\""
        ),
        ValueView::Bool(true)
    );
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
        Opcode::GetGlobal,
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
    assert_eq!(num("Object = 1; Object += 1; Object++; Object"), 3.0);
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
    assert_eq!(eval("let a = [1];\na[[]];"), ValueView::Undefined);
    // The prototype chain: Array.prototype, then Object.prototype. Its
    // push and toString are supported; the constructor still fails explicitly.
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
    assert_eq!(num("let a = [];\na.constructor().length"), 0.0);
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

    // Array patterns now consume the array's iterator (clj-quickjs-131).
    for src in ["let a; [a] = [1]; a", "let [a] = [1]; a"] {
        assert_eq!(eval(src), ValueView::Number(1.0), "{src}");
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
    for src in [
        "let o = {}; Object.defineProperty(o, 'k', {get: () => 1}); o.k === 1",
        "let o = {}; Object.defineProperty(o, 'k', {set: undefined}); o.k === undefined",
        "let o = {k: 1}; Object.defineProperty(o, 'k', {get: undefined}); o.k === undefined",
        "let o = {}; Object.defineProperty(o, {}, {value: 1}); o['[object Object]'] === 1",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
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
    assert_eq!(num("Object.defineProperty([1], 0, {get: () => 2})[0]"), 2.0);
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
    truth(
        "let e; try { null.x; } catch (x) { e = x; } Object.getPrototypeOf(e) === TypeError.prototype && Object.setPrototypeOf(e, null) === e && Object.getPrototypeOf(e) === null",
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
    // The shared intrinsic stores the built-in accessor (clj-quickjs-261).
    truth(
        "let d = Object.getOwnPropertyDescriptor(Object.prototype, '__proto__'); \
         typeof d.get === 'function' && typeof d.set === 'function' && \
         d.get.name === 'get __proto__' && d.set.name === 'set __proto__' && \
         !d.enumerable && d.configurable && !('value' in d) && !('writable' in d)",
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
    truth(
        "let e; try { null.x; } catch (caught) { e = caught; } !Object.isFrozen(e) && Object.isExtensible(e) && Object.freeze(e) === e && Object.isFrozen(e)",
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
    assert_eq!(
        listed("", "Object.getOwnPropertyNames(Object.prototype)"),
        "4:toString,valueOf,constructor,__proto__,"
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
        // A primitive's wrapper (clj-quickjs-142): a String's indices,
        // then its non-enumerable `length`; Number and Boolean have none.
        let (ab, empty) = match call {
            "Object.keys" => ("2:0,1,", "0:"),
            _ => ("3:0,1,length,", "1:length,"),
        };
        assert_eq!(listed("", &format!("{call}('ab')")), ab, "{call}");
        assert_eq!(listed("", &format!("{call}('')")), empty, "{call}");
        assert_eq!(listed("", &format!("{call}(1)")), "0:", "{call}");
        assert_eq!(listed("", &format!("{call}(true)")), "0:", "{call}");
    }
    truth("let k; try { null.x; } catch (e) { k = Object.keys(e); } k.length === 0");

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
    truth("let r; try { null.x; } catch (e) { r = delete e.message && e.message === ''; } r");

    // `delete x` is a compile-time SyntaxError in strict code.
    for src in ["function f() { delete x; }", "let x = 1; delete x;"] {
        let e = compile_script("t", src).unwrap_err();
        assert_eq!(e.kind, SyntaxErrorKind::Invalid, "{src}: {e}");
    }
    assert_eq!(sloppy_eval("delete x;"), ValueView::Bool(true));

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

    // Explicitly unsupported until their pellet: boolean properties (clj-quickjs-39).
    // Not catchable: no JavaScript sees a wrong answer.
    {
        let (src, needle) = ("let b = true;\nb.toString();", "of a boolean");
        let (kind, message, line, _) = failure_at(src);
        assert_eq!(kind, VmErrorKind::UnsupportedOperand, "{src}: {message}");
        assert!(message.contains(needle), "{src}: {message}");
        assert_eq!(line, 2, "{src}");
    }
    truth("let n = 1; n.toString(2) === '1' && n.toString('10') === '1'");
    truth("let r = 0; try { (1).toString(2); } catch (e) { r = 1; } r === 0");

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
        "Array.prototype.push.call(new String('ab'))",
        "Array.prototype.push.call(new String('ab'), 'c')",
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
        num("Array.prototype.push.call({length: {valueOf: function () { return 1; }}}, 7)"),
        2.0
    );

    assert_eq!(
        failure("let a = []; a.length = 4294967295; a.push(7)").kind(),
        VmErrorKind::RangeError
    );
    assert_eq!(num("Array().length"), 0.0);

    // A String wrapper permits ordinary index additions, then rejects the
    // final length Set even when its value is unchanged. Push always throws,
    // including in sloppy code, and preserves any earlier successful Sets.
    for src in [
        "let w = new String('ab'); let caught = false; try { Array.prototype.push.call(w); } catch(e) { caught = true; } caught && w.length === 2 && w[0] === 'a' && w[1] === 'b' && !(2 in w)",
        "let w = new String('ab'); let caught = false; try { Array.prototype.push.call(w, 'c'); } catch(e) { caught = true; } caught && w.length === 2 && w[0] === 'a' && w[1] === 'b' && w[2] === 'c'",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
        assert_eq!(sloppy_eval(src), ValueView::Bool(true), "{src}");
    }

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

/// String exotic descriptors and ordinary wrapper properties coexist.
/// Synthetic fixtures, not light-cljs output.
#[test]
fn primitive_wrappers_string_properties() {
    for src in [
        "let s = new String('abc'); s[0] === 'a' && s[3] === undefined && s['01'] === undefined && s[-1] === undefined && s[1.5] === undefined",
        "let s = new String('abc'); let d = Object.getOwnPropertyDescriptor(s, '0'); d.value === 'a' && !d.writable && d.enumerable && !d.configurable",
        "let s = new String('abc'); let d = Object.getOwnPropertyDescriptor(s, 'length'); d.value === 3 && !d.writable && !d.enumerable && !d.configurable",
        "let s = new String('abc'); Object.defineProperty(s, '0', {}); Object.defineProperty(s, '0', {value: 'a', writable: false, enumerable: true, configurable: false}) === s && s[0] === 'a'",
        "let s = new String('abc'); Object.defineProperty(s, 'length', {value: 3, writable: false, enumerable: false, configurable: false}) === s && s.length === 3",
        "let s = new String('abc'); Object.preventExtensions(s); Object.defineProperty(s, '0', {value: 'a'}); Object.defineProperty(s, 'length', {}); Object.isFrozen(s) && s.length === 3",
        "let s = new String('abc'); s.x = 4; Object.freeze(s); Object.isFrozen(s) && s[0] === 'a' && s.x === 4",
        "let s = new String('ab'); s.z = 1; s[5] = 5; s[2] = 2; s.a = 3; let k = Object.getOwnPropertyNames(s); k.length === 7 && k[0] === '0' && k[1] === '1' && k[2] === '2' && k[3] === '5' && k[4] === 'length' && k[5] === 'z' && k[6] === 'a'",
        "let s = new String('ab'); s.x = 1; Object.defineProperty(s, 'hidden', {value: 2}); let k = Object.keys(s); k.length === 3 && k[0] === '0' && k[1] === '1' && k[2] === 'x'",
        "let s = new String('ab'); s['01'] = 7; s[2] = 'c'; delete s[2] && delete s['01'] && s[2] === undefined && s.length === 2",
        "let s = ''; for (let k in new String('abc')) { s = s + k; } s === '012'",
        "let k = Object.getOwnPropertyNames(new String('')); k.length === 1 && k[0] === 'length'",
        "let k = Object.getOwnPropertyNames(String.prototype); k[0] === 'length' && k[1] === 'toString' && k[4] === 'charAt' && k[5] === 'charCodeAt' && k[6] === 'codePointAt' && k[7] === 'indexOf' && k[8] === 'lastIndexOf' && k[9] === 'substring' && k[10] === 'slice' && k[11] === 'concat' && k[12] === 'padStart' && k[13] === 'padEnd' && k[14] === 'split' && k[15] === 'replace' && k[16] === 'replaceAll' && k.length === 17",
        "'length' in new String('abc') && 0 in new String('abc') && !(3 in new String('abc'))",
        "let s = new String('abc'); Object.setPrototypeOf(s, null); s.length === 3 && s[1] === 'b' && s.toString === undefined",
        "Object.create(new String('abc')).length === 3 && Object.create(new String('abc'))[1] === 'b'",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
    for operation in [
        "s.length = 5",
        "s[0] = 'x'",
        "delete s.length",
        "delete s[0]",
        "Object.defineProperty(s, '0', {value: 'x'})",
        "Object.defineProperty(s, '0', {value: 97})",
        "Object.defineProperty(s, '0', {writable: true})",
        "Object.defineProperty(s, '0', {enumerable: false})",
        "Object.defineProperty(s, '0', {configurable: true})",
        "Object.defineProperty(s, '0', {get: undefined})",
        "Object.defineProperty(s, 'length', {value: 5})",
        "Object.defineProperty(s, 'length', {writable: true})",
        "Object.defineProperty(s, 'length', {enumerable: true})",
        "Object.defineProperty(s, 'length', {configurable: true})",
    ] {
        let src = format!("let s = new String('ab'); {operation}");
        assert_eq!(failure(&src).kind(), VmErrorKind::TypeError, "{src}");
        let caught = format!(
            "let s = new String('ab'); let caught = false; try {{ {operation}; }} catch (e) {{ caught = true; }} caught && s.length === 2 && s[0] === 'a'"
        );
        assert_eq!(eval(&caught), ValueView::Bool(true), "{caught}");
    }
    let sloppy = compile_script_with("string wrapper sloppy", "let s = new String('ab'); s.length = 5; s[0] = 'z'; s.length === 2 && s[0] === 'a' && delete s.length === false && delete s[0] === false", ScriptOptions::SLOPPY).unwrap();
    match run(&sloppy, &[]) {
        RunOutcome::Complete(v) => assert_eq!(v.view(), ValueView::Bool(true)),
        other => panic!("expected Complete, got {other:?}"),
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
        match (options, run(&script, &[])) {
            (ScriptOptions::STRICT, RunOutcome::Failed(error)) => {
                assert_eq!(error.kind(), VmErrorKind::TypeError);
            }
            (ScriptOptions::SLOPPY, RunOutcome::Complete(value)) => {
                assert_eq!(value.string().unwrap().to_string(), "a");
            }
            (_, other) => panic!("unexpected inherited write outcome: {other:?}"),
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
          && s.peer.child.v === 7 && s.toString() === '\uD800x'
          && s.length === 2 && s[0] === '\uD800' && s[1] === 'x'");
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
        ("isNaN(argument())", "false"),
        ("isFinite(argument())", "true"),
        ("isNaN.call(null, argument())", "false"),
        ("isFinite.apply(null, [argument()])", "true"),
        ("Reflect.apply(isFinite, null, [argument()])", "true"),
        ("isNaN.bind(null, argument())()", "false"),
        ("__rt.reduce(isFinite, argument(), __rt.vector(0))", "true"),
        ("isFinite(wrapper())", "true"),
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

/// A string's own `length` and integer-index properties (clj-quickjs-142;
/// ECMAScript String exotic objects; QuickJS `js_string_get_own_property`),
/// read on a primitive base without allocating a wrapper, or on the
/// immutable [[StringData]] of a constructed String wrapper.
/// Synthetic fixture, not light-cljs output; the `String("abc")` and
/// `"\u{10ffff}"` rows follow `research/quickjs/tests/test_builtin.js`
/// `test_string` (line 235, revision 7d1c13c5d8f8); see
/// `docs/architecture.md`, "QuickJS references and provenance". The
/// notice on `object_extensibility` covers them.
#[test]
fn string_length_and_index_properties() {
    let truth = |src: &str| assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    let undefined = |src: &str| assert_eq!(eval(src), ValueView::Undefined, "{src}");

    // The pellet's acceptance rows on primitive bases.
    for (src, want) in [
        ("\"abc\"[1]", "b"),
        ("\"abc\"[\"1\"]", "b"),
        ("let s = 'abc'; let i = 2; s[i] + s[0]", "ca"),
    ] {
        assert_eq!(string(src), want, "{src}");
    }
    for src in ["\"abc\"[3]", "\"abc\"[-1]", "\"abc\"[1.5]"] {
        undefined(src);
    }
    assert_eq!(eval("\"abc\".length"), ValueView::Number(3.0));
    assert_eq!(eval("\"\".length"), ValueView::Number(0.0));
    assert_eq!(eval("\"\\u{10ffff}\".length"), ValueView::Number(2.0));
    truth("\"\\u{10ffff}\"[0] === \"\\u{dbff}\" && \"\\u{10ffff}\"[1] === \"\\u{dfff}\"");
    truth(
        "let d = Object.getOwnPropertyDescriptor(\"ab\", \"0\"); \
         d.value === \"a\" && d.writable === false && d.enumerable === true && \
         d.configurable === false",
    );

    // Keys that are not canonical indices below the length are not own
    // properties and continue at String.prototype.
    for src in [
        "'abc'['01']",
        "'abc'['-0']",
        "'abc'['1.0']",
        "'abc'[4294967295]",
        "''[0]",
        "'abc'.x",
    ] {
        undefined(src);
    }
    truth("'abc'.toString === String.prototype.toString && 'abc'['length'] === 3");
    truth("let s = 'x'; s.length + s.length === 2 && `ab${1}`.length === 3");
    truth("('a' + 'bc')[2] === 'c' && ('a' + 'bc').length === 3");
    // A supplementary character is two code units, each a lone surrogate.
    truth("let s = 'a\\u{1F600}b'; s.length === 4 && s[3] === 'b' && s[1] + s[2] === '\\u{1F600}'");

    // Descriptors: `length` is read-only, non-enumerable, non-configurable;
    // keys the wrapper lacks give undefined; Number and Boolean wrappers
    // have no own properties.
    truth(
        "let d = Object.getOwnPropertyDescriptor('abc', 'length'); \
         d.value === 3 && !d.writable && !d.enumerable && !d.configurable",
    );
    truth("let d = Object.getOwnPropertyDescriptor('abc', 2); d.value === 'c' && d.enumerable");
    for src in [
        "Object.getOwnPropertyDescriptor('abc', 3)",
        "Object.getOwnPropertyDescriptor('abc', 'toString')",
        "Object.getOwnPropertyDescriptor(1, 'k')",
        "Object.getOwnPropertyDescriptor(true, 'length')",
    ] {
        undefined(src);
    }
    for src in [
        "Object.getOwnPropertyDescriptor(null, 'k')",
        "Object.getOwnPropertyDescriptor(undefined, 0)",
    ] {
        assert_eq!(failure(src).kind(), VmErrorKind::TypeError, "{src}");
    }

    // Own keys: the indices ascending, then `length` (not enumerable).
    truth(
        "let n = Object.getOwnPropertyNames('ab'); \
         n.length === 3 && n[0] === '0' && n[1] === '1' && n[2] === 'length'",
    );
    truth("let k = Object.keys('ab'); k.length === 2 && k[0] === '0' && k[1] === '1'");
    truth("let s = ''; for (let k in 'xyz') { s = s + k; } s === '012'");

    // Writes and deletes of the own properties are rejected as on the
    // wrapper: a catchable TypeError in strict code, nothing in sloppy code.
    for src in [
        "let s = 'ab'; s.length = 5",
        "let s = 'ab'; s[0] = 'z'",
        "let s = 'ab'; delete s.length",
        "let s = 'ab'; delete s[1]",
    ] {
        let caught = format!(
            "let r = false; try {{ {src}; }} catch (e) {{ r = true; }} r && 'ab'.length === 2"
        );
        truth(&caught);
    }
    truth("let s = 'ab'; delete s[2] && delete s.x");
    let sloppy = compile_script_with(
        "t",
        "let s = 'ab'; s.length = 5; s[0] = 'z'; \
         s.length === 2 && s[0] === 'a' && delete s.length === false",
        ScriptOptions::SLOPPY,
    )
    .unwrap();
    match run(&sloppy, &[]) {
        RunOutcome::Complete(v) => assert_eq!(v.view(), ValueView::Bool(true)),
        other => panic!("expected Complete, got {other:?}"),
    }
    // `in` needs an object: a string primitive is a TypeError.
    assert_eq!(failure("0 in 'ab'").kind(), VmErrorKind::TypeError);

    // The wrapper rows use the same own properties over [[StringData]].
    for src in [
        "String('abc').length === 3",
        "(new String('abc')).length === 3 && (new String('abc'))[2] === 'c'",
        "let w = new String('ab'); let caught = false; try { w.length = 5; } catch (e) { caught = true; } caught && w.length === 2",
        "let n = Object.getOwnPropertyNames(new String('ab')); n.length === 3 && n[0] === '0' && n[1] === '1' && n[2] === 'length'",
        r"let w = new String('\u{10ffff}'); w.length === 2 && w[0] === '\u{dbff}' && w[1] === '\u{dfff}'",
    ] {
        truth(src);
    }

    // The `docs/grammar.md` example lines, each true.
    for src in [
        "\"abc\".length === 3 && \"abc\"[1] === \"b\" && \"abc\"[\"1\"] === \"b\" && \"abc\"[3] === undefined;",
        "\"\\u{10ffff}\".length === 2 && \"\\u{10ffff}\"[0] === \"\\u{dbff}\" && Object.getOwnPropertyNames(\"ab\")[2] === \"length\";",
    ] {
        truth(src);
    }
}

/// String `length` and index reads allocate no heap object
/// (clj-quickjs-142): the live-object count is unchanged by a loop of them.
/// Synthetic fixture, not light-cljs output.
#[test]
fn string_index_reads_allocate_no_wrappers() {
    let rt = Runtime::new();
    let warm = exe("Number.prototype.toString(); 0");
    assert!(matches!(
        rt.spawn(&warm, &[]).unwrap().run().unwrap(),
        RunOutcome::Complete(_)
    ));
    rt.collect().unwrap();
    let before = rt.heap_stats().objects;
    let src = "let s = 'abcdefghij'; let r = 0; \
               for (let i = 0; i < 200; i++) { if (s[i % s.length] === 'c') { r = r + 1; } } r";
    match rt.spawn(&exe(src), &[]).unwrap().run().unwrap() {
        RunOutcome::Complete(v) => assert_eq!(v.view(), ValueView::Number(20.0)),
        other => panic!("expected Complete, got {other:?}"),
    }
    assert_eq!(rt.heap_stats().objects, before);
}

/// The descriptor sequence adapts QuickJS `test_global_var_opt`,
/// research/quickjs/tests/test_language.js at 7d1c13c5d8f8. Other rows are
/// synthetic fixtures, not light-cljs output. QuickJS's license:
///
/// Copyright (c) 2017-2021 Fabrice Bellard
/// Copyright (c) 2017-2021 Charlie Gordon
///
/// Permission is hereby granted, free of charge, to any person obtaining a
/// copy of this software and associated documentation files (the "Software"),
/// to deal in the Software without restriction, including without limitation
/// the rights to use, copy, modify, merge, publish, distribute, sublicense,
/// and/or sell copies of the Software, and to permit persons to whom the
/// Software is furnished to do so, subject to the following conditions:
/// The above copyright notice and this permission notice shall be included
/// in all copies or substantial portions of the Software.
/// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
/// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
/// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL
/// THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
/// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
/// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
/// DEALINGS IN THE SOFTWARE.
#[test]
fn global_object_bindings() {
    // Synthetic fixtures, not light-cljs output.
    for src in [
        "globalThis === this",
        "var a = 1; globalThis.a === 1",
        "function g() {} globalThis.g === g",
        "let b = 1; globalThis.b === undefined",
        "var a = 1; function f() { a = a + 1; return a; } f() === 2 && globalThis.a === 2",
        "var a; Object.getOwnPropertyDescriptor(globalThis, 'a').configurable === false",
        "function f() {} Object.getOwnPropertyDescriptor(globalThis, 'f').configurable === false",
        "Object.defineProperty(globalThis, 'x', {value: 1, writable: true, configurable: true}); function f() { return x; } f() === 1 && (globalThis.x = 2) === 2 && f() === 2",
        "Object.defineProperty(globalThis, 'x', {value: 1}); typeof x === 'number' && typeof missing === 'undefined'",
        "Object = 3; Object === 3 && globalThis.Object === 3",
        "var g = 1; function f() { return g; } globalThis.f() === 1",
        "globalThis === (() => this)()",
        "let original = this; globalThis = 7; this === original",
        "var Object; typeof Object === 'function'",
        "function f() { return f; } let original = f; globalThis.f = 3; original() === 3",
        "let calls = 0; Object.defineProperty(globalThis, 'x', {get() { calls++; return this === globalThis ? 7 : 0; }}); x === 7 && typeof x === 'number' && calls === 2",
        "Object.defineProperty(globalThis, 'x', {set(v) { this.saved = v; return 99; }}); x === undefined && typeof x === 'undefined' && (x = 3) === 3 && saved === 3",
        "let p = {get x() { return this.saved; }, set x(v) { this.saved = v; }}; Object.setPrototypeOf(globalThis, p); (x = 7) === 7 && x === 7 && globalThis.saved === 7",
        "let hits = 0; Object.defineProperty(globalThis, 'x', {get() { hits++; return 2; }, set(v) { this.saved = v; }}); let old = x++; old === 2 && saved === 3 && hits === 1",
        "let caught = 0; Object.defineProperty(globalThis, 'x', {get() { throw 7; }, set(v) { throw v; }}); try { x; } catch(e) { caught += e; } try { x = 3; } catch(e) { caught += e; } caught === 10",
        "Object.defineProperty(globalThis, 'x', {value: undefined, configurable: true}); let found = x === undefined; delete globalThis.x; let caught = false; try { x; } catch(e) { caught = true; } found && caught && typeof x === 'undefined'",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
    assert_eq!(failure("missing").kind(), VmErrorKind::ReferenceError);
    assert_eq!(failure("missing = 1").kind(), VmErrorKind::ReferenceError);
    assert_eq!(
        failure("Object.defineProperty(globalThis, 'gvar1', {value: 1}); gvar1 = 2").kind(),
        VmErrorKind::TypeError
    );
    assert_eq!(
        failure("Object.defineProperty(globalThis, 'x', {get() { return 1; }}); x = 2").kind(),
        VmErrorKind::TypeError
    );
    let sloppy = compile_script_with("globals", r#"
        Object.defineProperty(globalThis, 'gvar1', {value: 0, writable: true, configurable: true});
        function test() {
            var v2;
            gvar1 = 1;
            Object.defineProperty(globalThis, 'gvar1', {writable: false});
            gvar1 = 2;
            var ok = gvar1 === 1;
            Object.defineProperty(globalThis, 'gvar1', {
                get: function() { return this === globalThis ? 'hello' : 'wrong receiver'; },
                set: function(v) { v2 = this === globalThis ? v : 0; }
            });
            ok = ok && gvar1 === 'hello';
            gvar1 = 3;
            ok = ok && v2 === 3;
            Object.defineProperty(globalThis, 'gvar1', {value: 4, writable: true, configurable: true});
            ok = ok && gvar1 === 4;
            gvar1 = 6;
            Object.defineProperty(globalThis, 'getterOnly', {get: function() { return 9; }});
            getterOnly = 10;
            return ok && gvar1 === 6 && getterOnly === 9;
        }
        test()
    "#, ScriptOptions::SLOPPY).unwrap();
    assert!(
        matches!(run(&sloppy, &[]), RunOutcome::Complete(v) if v.view() == ValueView::Bool(true))
    );
}

/// Synthetic fixtures, not light-cljs output (clj-quickjs-105).
#[test]
fn sloppy_implicit_globals_and_delete() {
    for src in ["delete __rt;", "delete (__rt);"] {
        let error = compile_script_with("sloppy", src, ScriptOptions::SLOPPY).unwrap_err();
        assert_eq!(error.kind, SyntaxErrorKind::ReservedWord, "{src}: {error}");
        assert_eq!((error.line, error.column), (1, 1), "{src}: {error}");
    }
    assert_eq!(
        sloppy_eval("function f() { u = 3; } f(); u"),
        ValueView::Number(3.0)
    );
    assert_eq!(
        sloppy_eval("function f() { u = 3; } f(); globalThis.u"),
        ValueView::Number(3.0)
    );
    assert_eq!(sloppy_eval("u = 3; delete u"), ValueView::Bool(true));
    assert_eq!(
        sloppy_eval("u = 3; delete u; u = 5; u"),
        ValueView::Number(5.0)
    );
    for src in [
        "u = 3; let d = Object.getOwnPropertyDescriptor(globalThis, 'u'); d.value === 3 && d.writable && d.enumerable && d.configurable",
        "var v = 1; delete v === false && v === 1",
        "function f() {} delete f === false && typeof f === 'function'",
        "let w = 1; delete w === false && w === 1",
        "const w = 1; delete w === false && w === 1",
        "delete nothing_here",
        "delete eval",
        "let eval = 1; delete eval === false && eval === 1",
        "function f() { return delete arguments; } f() === false",
        "let ok = delete w === false; let w = 1; ok && w === 1",
        "function f(p) { var v = 1; let w = 2; return delete p === false && delete v === false && delete w === false; } f(3)",
        "let w = 1; function f() { return delete w; } f() === false && w === 1",
        "let f = function self() { return delete self; }; f() === false",
        "let hits = 0; Object.defineProperty(globalThis, 'u', {get() { hits++; }, configurable: true}); delete u && hits === 0 && typeof u === 'undefined'",
        "Object.defineProperty(globalThis, 'u', {value: 1}); delete u === false && u === 1",
        "let p = {u: 1}; Object.setPrototypeOf(globalThis, p); delete u && u === 1 && p.u === 1",
        "delete undefined === false && delete NaN === false && delete Infinity === false",
        "delete (/* comment */ Infinity) === false && delete \\u0075ndefined === false",
        "NaN; delete (/* NaN */ 1) && delete (NaN, 1)",
        "u = 1; u += 2; let old = u++; old === 3 && globalThis.u === 4 && delete u",
        "for (k in {a: 1}) {} k === 'a' && delete k",
        "Object.preventExtensions(globalThis); u = 1; typeof u === 'undefined'",
        "let g = globalThis; globalThis = null; u = 1; g.u === 1 && delete u && typeof u === 'undefined'",
    ] {
        assert_eq!(sloppy_eval(src), ValueView::Bool(true), "{src}");
    }
    for src in [
        "u = 3; delete u; u",
        "function f() { 'use strict'; u2 = 1; } f()",
        "missing += 1",
        "missing++",
    ] {
        let program = compile_script_with("sloppy", src, ScriptOptions::SLOPPY).unwrap();
        match run(&program, &[]) {
            RunOutcome::Failed(e) => assert_eq!(e.kind(), VmErrorKind::ReferenceError, "{src}"),
            other => panic!("{src}: expected ReferenceError, got {other:?}"),
        }
    }
}

#[test]
fn global_bytecode_round_trip_and_name_validation() {
    let program = exe("var a = 1; function f() { return a; } globalThis.a++; typeof missing; f()");
    let reloaded = Executable::load(program.to_spec()).unwrap();
    assert!(
        matches!(run(&reloaded, &[]), RunOutcome::Complete(v) if v.view() == ValueView::Number(2.0))
    );
    for op in [
        Opcode::GetGlobal,
        Opcode::GetGlobalOrUndefined,
        Opcode::PutGlobal,
        Opcode::DeclareGlobalVar,
        Opcode::DeclareGlobalFunction,
    ] {
        let mut spec = program.to_spec();
        spec.code = vec![op as u8, 0, 0, Opcode::Return as u8];
        spec.constants = vec![Primitive::Number(1.0)];
        assert_eq!(
            Executable::load(spec).unwrap_err().kind,
            ValidationErrorKind::NameNotString { index: 0 }
        );
    }
}

/// Synthetic fixtures, not light-cljs output.
#[test]
fn global_object_descriptors() {
    let truth = |src: &str| assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    truth(
        "let d = Object.getOwnPropertyDescriptor(globalThis, 'globalThis'); \
         d !== undefined && d.value === this && d.writable && !d.enumerable && d.configurable \
         && Object.keys(this).length === 0",
    );
    for name in [
        "Object",
        "Number",
        "String",
        "Array",
        "Function",
        "Reflect",
        "isNaN",
        "isFinite",
        "parseInt",
        "parseFloat",
        "Math",
    ] {
        let value_check = match name {
            "Reflect" => {
                "typeof d.value === 'object' && typeof d.value.apply === 'function'".to_string()
            }
            "Math" => {
                "typeof d.value === 'object' && typeof d.value.floor === 'function'".to_string()
            }
            _ => format!("typeof d.value === 'function' && d.value.name === '{name}'"),
        };
        truth(&format!(
            "let d = Object.getOwnPropertyDescriptor(globalThis, '{name}'); \
             d !== undefined && d.value === {name} && {value_check} \
             && d.writable && !d.enumerable && d.configurable"
        ));
    }
    for (name, check) in [
        ("undefined", "d.value === undefined"),
        ("NaN", "Object.is(d.value, NaN)"),
        ("Infinity", "d.value === Infinity"),
    ] {
        truth(&format!(
            "let d = Object.getOwnPropertyDescriptor(globalThis, '{name}'); \
             d !== undefined && {check} && !d.writable && !d.enumerable && !d.configurable"
        ));
    }
    truth(
        "var a; function g() {} \
         let d = Object.getOwnPropertyDescriptor(globalThis, 'a'); \
         let f = Object.getOwnPropertyDescriptor(globalThis, 'g'); \
         d !== undefined && d.value === undefined && d.writable && d.enumerable && !d.configurable \
         && f !== undefined && f.value === g && f.writable && f.enumerable && !f.configurable",
    );
    truth(
        "Object.defineProperty(globalThis, 'globalThis', {enumerable: true}); \
         Object.keys(this)[0] === 'globalThis'",
    );
}

/// Synthetic fixtures, not light-cljs output (clj-quickjs-156).
/// Basic join/toString rows cover QuickJS tests/test_builtin.js::test_array,
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
fn array_join_and_to_string() {
    for (src, expected) in [
        ("[1, 2, 3, 4].join()", "1,2,3,4"),
        ("[1, 2].join('-')", "1-2"),
        ("[1, 2].join(undefined)", "1,2"),
        ("[].join()", ""),
        ("[null, undefined, 1].join()", ",,1"),
        ("[1, , 3].join()", "1,,3"),
        ("[[1, 2], [3]].join(';')", "1,2;3"),
        ("[1, 2].toString()", "1,2"),
        ("'' + [1, 2]", "1,2"),
        ("`${[1, 2]}`", "1,2"),
        ("[{}].join()", "[object Object]"),
        ("[{toString: function () { return 'x'; }}].join()", "x"),
        ("let b = [1]; b.join = 5; b.toString()", "[object Array]"),
        (
            "Array.prototype.join.call({0: 'a', 2: 'c', length: '3.9'}, '-')",
            "a--c",
        ),
        ("Array.prototype.join.call('abc', '-')", "a-b-c"),
        ("Array.prototype.join.call(3)", ""),
        ("Array.prototype.join.call({length: -2})", ""),
        ("Array.prototype.join.call({length: NaN})", ""),
        (
            "Array.prototype.join.call({length: {valueOf() { return 2; }}, 0: 4, 1: 5})",
            "4,5",
        ),
        (
            "Array.prototype.toString.call({join() { return this.x; }, x: 'ok'})",
            "ok",
        ),
        (
            "Array.prototype.toString.call({join: null})",
            "[object Object]",
        ),
        (
            "let a = [1, 2]; Object.defineProperty(a, 'join', {get() { return function () { return this[1] + 'x'; }; }}); a.toString()",
            "2x",
        ),
        (
            "let a = [, 2]; let p = Object.create(Array.prototype); p[0] = 7; Object.setPrototypeOf(a, p); a.join()",
            "7,2",
        ),
        (
            "let a = [1, 2]; a[0] = {toString() { a.push(3); a[1] = 4; return 'x'; }}; a.join()",
            "x,4",
        ),
        ("[1, 2].join({toString() { return '-'; }})", "1-2"),
        ("[1, 2].join(null)", "1null2"),
    ] {
        assert_eq!(string(src), expected, "{src}");
    }
    for src in [
        "let log = ''; let o = {get length() { log += 'l'; return {valueOf() { log += 'v'; return 2; }}; }, get 0() { log += 'a'; return {toString() { log += 't'; return 'x'; }}; }, get 1() { log += 'b'; return null; }}; let s = {toString() { log += 's'; return '-'; }}; Array.prototype.join.call(o, s) === 'x-' && log === 'lvsatb'",
        "let hit = 0; [].join({toString() { hit++; return ''; }}) === '' && hit === 1",
        "let o = {join() { return 42; }}; Array.prototype.toString.call(o) === 42",
        "Array.prototype.join.name === 'join' && Array.prototype.join.length === 1 && Array.prototype.toString.length === 0",
        "let d = Object.getOwnPropertyDescriptor(Array.prototype, 'join'); d.writable && !d.enumerable && d.configurable",
        "let caught = false; try { [1, {toString() { throw 42; }}].join(); } catch (e) { caught = e === 42; } caught && [2, 3].join() === '2,3'",
        "let a = [1]; a[1] = a; let caught = false; try { a.join(); } catch (e) { caught = true; } caught && [3].join() === '3'",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
    for src in [
        "Array.prototype.join.call(null)",
        "Array.prototype.join.call(undefined)",
        "Array.prototype.toString.call(null)",
        "Array.prototype.toString.call(undefined)",
        "[Object.create(null)].join()",
    ] {
        assert_eq!(failure(src).kind(), VmErrorKind::TypeError, "{src}");
    }
    assert_eq!(
        failure("let a = [1]; a[1] = a; a.join()").kind(),
        VmErrorKind::StackOverflow
    );
}

#[test]
fn array_join_yields_and_collects() {
    struct JoinHost(bool);
    impl Host for JoinHost {
        fn effect(&mut self, call: HostCall) -> Result<Primitive, String> {
            self.0 = true;
            Ok(call.arg)
        }
    }
    // Synthetic fixture, not light-cljs output. Budget during both element
    // conversion and copying a single large element/separator.
    for (source, minimum_yields) in [
        (
            "let a = []; for (let i = 0; i < 10000; i++) { a.push('x'); } __rt.effect(1, 0); let s = a.join(); s.length === 19999 && s[19998] === 'x'",
            100,
        ),
        (
            "let s = 'x'.padEnd(20000, 'y'); let a = [s, s]; __rt.effect(1, 0); let result = a.join(s); result.length === 60000 && result[59999] === 'y'",
            2,
        ),
    ] {
        let rt = Runtime::new();
        let task = rt.spawn(&exe(source), &[]).unwrap();
        let mut yields = 0;
        let mut complete = false;
        let mut host = JoinHost(false);
        for _ in 0..10000 {
            match task.run_with(Budget::instructions(100), &mut host).unwrap() {
                RunOutcome::Yielded => {
                    yields += usize::from(host.0);
                    rt.collect().unwrap();
                }
                RunOutcome::Complete(value) => {
                    assert_eq!(value.view(), ValueView::Bool(true), "{source}");
                    complete = true;
                    break;
                }
                other => panic!("unexpected {other:?}"),
            }
        }
        assert!(
            complete && yields >= minimum_yields,
            "completed={complete}, yields={yields}"
        );
    }
}

/// Synthetic fixtures, not light-cljs output (clj-quickjs-228).
#[test]
fn error_constructor() {
    for src in [
        "Error('hello').message === 'hello'",
        "new Error('a').message === 'a'",
        "Error().message === '' && Object.getOwnPropertyDescriptor(Error(undefined), 'message') === undefined",
        "Error(12).message === '12'",
        "new Error('m', {cause: 0}).cause === 0",
        "new Error('m', {}).cause === undefined && new Error('m', 5).cause === undefined",
        "Error.prototype.name === 'Error' && Error.prototype.message === '' && Error.prototype.constructor === Error",
        "Error.length === 1 && Error.name === 'Error' && Error.prototype.toString.length === 0 && Error.prototype.toString.name === 'toString'",
        "Object.getPrototypeOf(Error('x')) === Error.prototype && Error('x') instanceof Error",
        "Error('x').toString() === 'Error: x' && Error().toString() === 'Error' && '' + Error('x') === 'Error: x'",
        "Error.prototype.toString.call({name: '', message: 'm'}) === 'm'",
        "Error.prototype.toString.call({}) === 'Error' && Error.prototype.toString.call({name: '', message: ''}) === ''",
        "Error.prototype.toString.call({name: 'N', message: ''}) === 'N'",
        "let caught = false; try { Error.prototype.toString.call(1); } catch (e) { caught = true; } caught",
        "let c; try { throw new Error('getter'); } catch (e) { c = e; } c.message === 'getter' && c instanceof Error",
        "let e = Error('m', {cause: undefined}); let m = Object.getOwnPropertyDescriptor(e, 'message'); let c = Object.getOwnPropertyDescriptor(e, 'cause'); m.value === 'm' && m.writable && m.configurable && !m.enumerable && c.value === undefined && c.writable && c.configurable && !c.enumerable && Object.keys(e).length === 0",
        "let p = {cause: 42}; Error('m', Object.create(p)).cause === 42",
        "let log = ''; let options = {get cause() { log += 'c'; return this.n; }, n: 9}; let e = Error({toString() { log += 'm'; return 'text'; }}, options); log === 'mc' && e.message === 'text' && e.cause === 9",
        "let log = ''; let o = {get name() { log += 'n'; return {toString() { log += 'N'; return 'custom'; }}; }, get message() { log += 'm'; return {toString() { log += 'M'; return 'text'; }}; }}; Error.prototype.toString.call(o) === 'custom: text' && log === 'nNmM'",
        "let caught = false; try { Error({toString() { throw 42; }}, {get cause() { throw 43; }}); } catch (e) { caught = e === 42; } caught",
        "let caught = false; try { Error('m', {get cause() { throw 43; }}); } catch (e) { caught = e === 43; } caught",
        "let caught = false; try { Error.prototype.toString.call({get name() { throw 44; }, get message() { throw 45; }}); } catch (e) { caught = e === 44; } caught",
        "let E = Error.bind(null, 'bound'); new E().message === 'bound' && new E() instanceof Error",
        "let e = Error('m', {cause: 1}); e.message = 'n'; e.cause = 2; delete e.message; e.message === '' && e.cause === 2",
        "globalThis.Error === Error && Object.getPrototypeOf(Error.prototype) === Object.prototype",
        "Object.prototype.toString.call(Error()) === '[object Error]' && Object.prototype.toString.call(Object.create(Error.prototype)) === '[object Object]' && Object.prototype.toString.call(Error.prototype) === '[object Object]'",
        "let e = Error('x'); Object.setPrototypeOf(e, null); Object.prototype.toString.call(e) === '[object Error]'",
        "Error.call(null, 'x').message === 'x' && Error.apply(null, ['y']).message === 'y' && Reflect.apply(Error, null, ['z']).message === 'z'",
        "__rt.reduce(Error, 'first', __rt.vector(1, 2)).message === 'Error: first'",
        "let caught = false; try { Error.prototype.toString.call({name: {toString() { throw 42; }}, get message() { throw 43; }}); } catch (e) { caught = e === 42; } caught",
        "let caught = false; try { Error.prototype.toString.call({name: 'N', message: {toString() { throw 43; }}}); } catch (e) { caught = e === 43; } caught",
        "let caught = false; try { Error.prototype.message = 'changed'; } catch (e) { caught = true; } caught && Error().message === ''",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
    assert_eq!(
        failure("Error.prototype.toString.call(1)").kind(),
        VmErrorKind::TypeError
    );
}

/// Synthetic fixture, not light-cljs output: each callback can suspend,
/// yield, be collected and move threads, including nested ToString calls.
#[test]
fn error_callbacks_wait_yield_collect_and_migrate() {
    let rt = Runtime::new();
    let src = r"
        let log = '';
        let e = new Error({get toString() {
            log += 'g'; __rt.need(1, 1);
            return function() { log += 's'; __rt.need(2, 1); return 'text'; };
        }}, Object.create({get cause() {
            log += 'c'; __rt.need(3, 1); return {n: 42};
        }}));
        let message = e.message;
        let cause = e.cause;
        Object.defineProperty(e, 'name', {get() {
            log += 'n'; __rt.need(4, 1);
            return {toString() { log += 'N'; __rt.need(5, 1); return 'Custom'; }};
        }});
        Object.defineProperty(e, 'message', {get() {
            log += 'm'; __rt.need(6, 1);
            return {toString() { log += 'M'; __rt.need(7, 1); return message; }};
        }});
        e.toString() === 'Custom: text' && log === 'gscnNmM'
            && cause.n === 42 && e instanceof Error
    ";
    let mut task = rt.spawn(&exe(src), &[]).unwrap();
    let mut waited = 0;
    let mut complete = false;
    for _ in 0..1000 {
        let (next, outcome) = std::thread::spawn(move || {
            let outcome = task
                .run_with(Budget::instructions(1), &mut NoEffects)
                .unwrap();
            (task, outcome)
        })
        .join()
        .unwrap();
        task = next;
        match outcome {
            RunOutcome::Yielded => {
                rt.collect().unwrap();
            }
            RunOutcome::Waiting(_) => {
                waited += 1;
                assert!(waited <= 7);
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
            RunOutcome::Complete(value) => {
                assert_eq!(value.view(), ValueView::Bool(true));
                complete = true;
                break;
            }
            other => panic!("{other:?}"),
        }
    }
    assert!(complete);
    assert_eq!(waited, 7);
}

/// Synthetic fixture, not light-cljs output: Error instances retain the
/// ordinary-object publication policy, including dependency failures.
#[test]
fn error_objects_are_task_owned_and_cannot_be_published_as_failures() {
    use clj_quickjs::{CompleteError, PublishError};
    let rt = Runtime::new();
    let owner = rt
        .spawn(&exe("Error('owned', {cause: {n: 42}})"), &[])
        .unwrap();
    let RunOutcome::Complete(error) = owner.run().unwrap() else {
        panic!("expected Error");
    };
    assert!(matches!(
        rt.publish(&error),
        Err(PublishError::NotShareable { .. })
    ));
    let consumer = rt.spawn(&exe("__rt.need(228, 1)"), &[]).unwrap();
    assert!(matches!(consumer.run().unwrap(), RunOutcome::Waiting(_)));
    let request = consumer.take_producer_requests().pop().unwrap();
    assert!(matches!(
        rt.complete(
            request.key(),
            request.producer(),
            Completion::Failed(&error)
        ),
        Err(CompleteError::NotShareable(
            PublishError::NotShareable { .. }
        ))
    ));
    // Rejection leaves the cell pending, so a permitted failure can still
    // complete it and wake the original waiter.
    let safe = rt.root(Primitive::Number(9.0));
    let wakes = rt
        .complete(request.key(), request.producer(), Completion::Failed(&safe))
        .unwrap();
    assert_eq!(wakes.into_runnable().len(), 1);
    let reader = compile_function("function reader(e) { return e.message; }").unwrap();
    let intruder = rt.spawn(&reader, &[&error]).unwrap();
    assert!(matches!(intruder.run().unwrap(), RunOutcome::Failed(e)
        if matches!(e.kind(), VmErrorKind::ForeignObject { .. })));
    let weak = error.downgrade();
    rt.collect().unwrap();
    assert!(weak.upgrade().is_ok());
    drop((error, owner, intruder));
    rt.collect().unwrap();
    assert!(weak.upgrade().is_err());
}

/// Synthetic fixtures, not light-cljs output (clj-quickjs-229).
#[test]
fn native_error_constructors() {
    for name in [
        "TypeError",
        "RangeError",
        "ReferenceError",
        "SyntaxError",
        "EvalError",
        "URIError",
        "InternalError",
    ] {
        for expression in [
            "new N('m').message === 'm'",
            "N('m').name === 'NAME' && N('m').toString() === 'NAME: m'",
            "N.prototype.name === 'NAME' && N.prototype.message === '' && N.prototype.constructor === N",
            "Object.getPrototypeOf(N) === Error && Object.getPrototypeOf(N.prototype) === Error.prototype",
            "N.prototype.toString === Error.prototype.toString && Object.getOwnPropertyDescriptor(N.prototype, 'toString') === undefined",
            "N.name === 'NAME' && N.length === 1",
            "let p = Object.getOwnPropertyDescriptor(N.prototype, 'name'); let m = Object.getOwnPropertyDescriptor(N.prototype, 'message'); let c = Object.getOwnPropertyDescriptor(N.prototype, 'constructor'); p.value === 'NAME' && p.writable && p.configurable && !p.enumerable && m.value === '' && m.writable && m.configurable && !m.enumerable && c.value === N && c.writable && c.configurable && !c.enumerable",
            "let n = Object.getOwnPropertyDescriptor(N, 'name'); let l = Object.getOwnPropertyDescriptor(N, 'length'); n.value === 'NAME' && !n.writable && !n.enumerable && n.configurable && l.value === 1 && !l.writable && !l.enumerable && l.configurable",
            "new N('m', {cause: 1}).cause === 1",
            "new N() instanceof N && new N() instanceof Error && Object.getPrototypeOf(N()) === N.prototype",
            "N() !== N() && N().toString() === 'NAME' && N(12).message === '12'",
            "Object.getOwnPropertyDescriptor(N(undefined), 'message') === undefined && N(undefined).message === ''",
            "let e = N('m', {cause: undefined}); let m = Object.getOwnPropertyDescriptor(e, 'message'); let c = Object.getOwnPropertyDescriptor(e, 'cause'); m.value === 'm' && m.writable && m.configurable && !m.enumerable && c.value === undefined && c.writable && c.configurable && !c.enumerable && Object.keys(e).length === 0",
            "N('m', Object.create({cause: 42})).cause === 42 && N('m', 1).cause === undefined",
            "let log = ''; let e = N({toString() { log += 'm'; return 'text'; }}, {get cause() { log += 'c'; return 9; }}); log === 'mc' && e.message === 'text' && e.cause === 9",
            "let caught = false; try { N({toString() { throw 42; }}, {get cause() { throw 43; }}); } catch (e) { caught = e === 42; } caught",
            "let caught = false; try { new N('m', {get cause() { throw 43; }}); } catch (e) { caught = e === 43; } caught",
            "let B = N.bind(null, 'bound'); new B().message === 'bound' && new B() instanceof N && B().name === 'NAME'",
            "N.call(null, 'x').message === 'x' && N.apply(null, ['y']).message === 'y' && Reflect.apply(N, null, ['z']).message === 'z'",
            "let e = N('m', {cause: 1}); e.message = 'n'; e.cause = 2; delete e.message; e.message === '' && e.cause === 2",
            "Object.prototype.toString.call(N()) === '[object Error]' && Object.prototype.toString.call(N.prototype) === '[object Object]'",
            "let e; try { throw new N('caught'); } catch (caught) { e = caught; } e instanceof N && e.message === 'caught'",
            "globalThis.NAME === N && Object.getOwnPropertyDescriptor(globalThis, 'NAME').writable",
            "Error = 0; globalThis.NAME = 0; let e = new N('kept'); e instanceof N && e.name === 'NAME' && e.toString() === 'NAME: kept'",
        ] {
            let src = format!("const N = {name}; {}", expression.replace("NAME", name));
            assert_eq!(eval(&src), ValueView::Bool(true), "{src}");
        }
        let error = failure(&format!("throw new {name}('boom')"));
        assert_eq!(error.kind(), VmErrorKind::Thrown);
        assert_eq!(error.message(), format!("uncaught exception: {name}: boom"));
    }
    assert_eq!(
        eval("new TypeError() instanceof RangeError"),
        ValueView::Bool(false)
    );
    assert_eq!(
        eval("let TypeError = 42; TypeError === 42"),
        ValueView::Bool(true)
    );
    assert_eq!(
        eval("TypeError = 42; globalThis.TypeError === 42"),
        ValueView::Bool(true)
    );
}

/// Synthetic fixtures, not light-cljs output: all NativeErrors share Error's
/// resumable callbacks and retain their prototype across GC and migration.
#[test]
fn native_error_callbacks_wait_yield_collect_and_migrate() {
    for name in [
        "TypeError",
        "RangeError",
        "ReferenceError",
        "SyntaxError",
        "EvalError",
        "URIError",
        "InternalError",
    ] {
        let rt = Runtime::new();
        let src = format!(
            r"
            let log = '';
            let e = new {name}({{toString() {{ log += 'm'; __rt.need(1, 1); return 'text'; }}}},
                Object.create({{get cause() {{ log += 'c'; __rt.need(2, 1); return {{n: 42}}; }}}}));
            Object.defineProperty(e, 'name', {{get() {{ log += 'n'; __rt.need(3, 1); return '{name}'; }}}});
            Object.defineProperty(e, 'message', {{get() {{ log += 's'; __rt.need(4, 1); return 'text'; }}}});
            e.toString() === '{name}: text' && log === 'mcns'
                && e.cause.n === 42 && e instanceof {name} && e instanceof Error
        "
        );
        let mut task = rt.spawn(&exe(&src), &[]).unwrap();
        let mut waited = 0;
        let mut complete = false;
        for _ in 0..1000 {
            let (next, outcome) = std::thread::spawn(move || {
                let outcome = task
                    .run_with(Budget::instructions(1), &mut NoEffects)
                    .unwrap();
                (task, outcome)
            })
            .join()
            .unwrap();
            task = next;
            match outcome {
                RunOutcome::Yielded => {
                    rt.collect().unwrap();
                }
                RunOutcome::Waiting(_) => {
                    waited += 1;
                    assert!(waited <= 4, "{name}");
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
                RunOutcome::Complete(value) => {
                    assert_eq!(value.view(), ValueView::Bool(true), "{name}");
                    complete = true;
                    break;
                }
                other => panic!("{name}: {other:?}"),
            }
        }
        assert!(complete, "{name}");
        assert_eq!(waited, 4, "{name}");
    }
}

/// Synthetic fixtures, not light-cljs output. Identity/property rows adapt
/// QuickJS tests/test_builtin.js test_symbol (7d1c13c5d8f8); its MIT notice
/// is retained on string_code_unit_access above (see docs/attribution.md).
#[test]
fn symbol_values() {
    for src in [
        "String(Symbol()) === 'Symbol()' && String(Symbol(undefined)) === 'Symbol()' && String(Symbol('x')) === 'Symbol(x)' && String(Symbol(null)) === 'Symbol(null)' && String(Symbol(42)) === 'Symbol(42)'",
        "String(Symbol({toString() { return undefined; }})) === 'Symbol(undefined)' && String(Symbol({toString() { return '\\uD800'; }})) === 'Symbol(\\uD800)'",
        "let s = Symbol('k'); let t = Symbol('k'); let m = __rt.map(s, 3, t, 4); __rt.get(m, s) === 3 && __rt.get(m, t) === 4 && __rt.getOr(m, 'k', undefined) === undefined",
        "let a = Symbol('abc'); let b = Symbol('abc'); let obj = {}; obj[a] = 2; obj[a] === 2 && obj['abc'] === undefined && a == a && a === a && a != b && a !== b && typeof a === 'symbol'",
        "Symbol() !== Symbol() && Symbol('x') !== Symbol('x') && !!Symbol()",
        "let s = Symbol('x'); function tag(parts, value) { return value === s; } tag`${s}`",
        "Symbol.iterator === Symbol.iterator && Symbol.toPrimitive !== Symbol.hasInstance && typeof Symbol.iterator === 'symbol'",
        "let s = Symbol('x'); let o = {x: 1}; o[s] = 2; Object.keys(o).join() === 'x' && Object.getOwnPropertyNames(o).join() === 'x'",
        "let s = Symbol('x'); let o = {}; o[s] = 2; let names = ''; for (let k in o) { names += k; } names === '' && s in o && !('x' in o)",
        "let s = Symbol(); let a = []; a[s] = 3; a[s]++ === 3 && a[s] === 4 && a.length === 0 && delete a[s] && !(s in a)",
        "let s = Symbol(); function f() {} f[s] = 3; f[s] === 3 && Object.keys(f).length === 0 && delete f[s] && !(s in f)",
        "let s = Symbol(); let p = {}; Object.defineProperty(p, s, {get() { return 7; }}); let o = Object.create(p); o[s] === 7 && s in o && Object.getOwnPropertyNames(p).length === 0",
        "let s = Symbol(); let o = {}; Object.defineProperty(o, s, {value: 2, writable: true}); o[s]++; Object.getOwnPropertyDescriptor(o, s).value === 3",
        "let s = Symbol(); s != 1 && s != 'x' && s != true && s != null && s != undefined && Object.is(s, s) && !Object.is(s, Symbol())",
        "let n = 0; Symbol({toString() { n++; return 'k'; }}) !== Symbol('k') && n === 1",
        "let s = Symbol(); let key = {toString() { return s; }}; let o = {}; o[key] = 5; o[s] === 5",
        "let d = Object.getOwnPropertyDescriptor(Symbol, 'iterator'); !d.writable && !d.enumerable && !d.configurable && d.value === Symbol.iterator && Symbol.length === 0 && Symbol.name === 'Symbol'",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
    for src in [
        "new Symbol()",
        "'' + Symbol()",
        "Symbol() + 1",
        "Symbol() * 1",
        "Number(Symbol())",
        "Symbol(Symbol())",
        "new Error(Symbol())",
        "`${Symbol()}`",
        "String({toString() { return Symbol(); }})",
        "new String(Symbol())",
        "String.raw`x${Symbol()}y`",
        "String.raw({raw: [Symbol()]})",
    ] {
        assert_eq!(failure(src).kind(), VmErrorKind::TypeError, "{src}");
        let caught = format!(
            "let caught = false; try {{ {src}; }} catch (e) {{
               caught = e instanceof TypeError && e instanceof Error &&
                 Object.getPrototypeOf(e) === TypeError.prototype && e.name === 'TypeError';
             }} caught"
        );
        assert_eq!(eval(&caught), ValueView::Bool(true), "{src}");
    }
}

/// Synthetic fixtures, not light-cljs output. The first row adapts
/// QuickJS test_builtin.js::test_symbol at 7d1c13c5d8f8; its full MIT notice
/// is retained at string_code_unit_access in this file.
#[test]
fn symbol_registry() {
    for src in [
        "let a = Symbol('abc'); let b = Symbol.for('abc'); let c = Symbol.for('abc'); b === c && b !== a && Symbol.keyFor(b) === 'abc' && Symbol.keyFor(a) === undefined",
        "Symbol.for(1) === Symbol.for('1') && Symbol.for() === Symbol.for('undefined') && Symbol.for(null) === Symbol.for('null')",
        "Symbol.for('a' + 'bc') === Symbol.for('abc') && Symbol.for('') !== Symbol.for('undefined') && Symbol.keyFor(Symbol.for('')) === ''",
        "Symbol.keyFor(Symbol.iterator) === undefined && Symbol.keyFor(Symbol.toPrimitive) === undefined",
        "Symbol.for('\\uD800') === Symbol.for(String.fromCharCode(55296)) && Symbol.for('\\uD800') !== Symbol.for('\\uFFFD') && Symbol.keyFor(Symbol.for('\\uD800')).charCodeAt(0) === 55296",
        "let n = 0; Symbol.for({toString() { n++; return 'abc'; }}) === Symbol.for('abc') && n === 1",
        "Symbol.for({toString() { return undefined; }}) === Symbol.for('undefined')",
        "let log = ''; let s = Symbol.for({toString() { log += 's'; return {}; }, valueOf() { log += 'v'; return 12; }}); s === Symbol.for('12') && log === 'sv'",
        "Symbol.for.call(null, 'abc') === Symbol.for('abc') && Symbol.keyFor.call(null, Symbol.for('abc')) === 'abc'",
        "Symbol.for.length === 1 && Symbol.for.name === 'for' && Symbol.keyFor.length === 1 && Symbol.keyFor.name === 'keyFor'",
        "let d = Object.getOwnPropertyDescriptor(Symbol, 'for'); d.value === Symbol.for && d.writable && !d.enumerable && d.configurable",
        "let d = Object.getOwnPropertyDescriptor(Symbol, 'keyFor'); d.value === Symbol.keyFor && d.writable && !d.enumerable && d.configurable",
        "let caught = false; try { Symbol.keyFor(1); } catch (e) { caught = true; } caught",
        "let caught = false; try { Symbol.for({toString() { throw 42; }}); } catch (e) { caught = e === 42; } caught",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
    for src in [
        "Symbol.keyFor(1)",
        "Symbol.keyFor()",
        "Symbol.keyFor('abc')",
        "Symbol.keyFor({toString() { throw 42; }})",
        "Symbol.for(Symbol())",
        "Symbol.for({toString() { return Symbol(); }})",
        "new Symbol.for('abc')",
        "new Symbol.keyFor(Symbol.for('abc'))",
    ] {
        assert_eq!(failure(src).kind(), VmErrorKind::TypeError, "{src}");
    }
}

/// Synthetic fixture, not light-cljs output: registry key conversion may
/// wait, yield, collect and migrate before any registry lock is taken.
#[test]
fn symbol_registry_conversion_waits_and_migrates() {
    let rt = Runtime::new();
    let source = r"
        let log = '';
        let s = Symbol.for({get toString() {
            log += 'g'; __rt.need(1281, 1);
            return function() {
                log += 's'; __rt.need(1282, 1);
                return Symbol.keyFor(Symbol.for('nested'));
            };
        }});
        s === Symbol.for('nested') && log === 'gs'
    ";
    let mut task = rt.spawn(&exe(source), &[]).unwrap();
    let mut waited = 0;
    for _ in 0..1000 {
        let (next, outcome) = std::thread::spawn(move || {
            let outcome = task
                .run_with(Budget::instructions(1), &mut NoEffects)
                .unwrap();
            (task, outcome)
        })
        .join()
        .unwrap();
        task = next;
        match outcome {
            RunOutcome::Yielded => {
                rt.collect().unwrap();
            }
            RunOutcome::Waiting(_) => {
                waited += 1;
                assert!(waited <= 2);
                rt.collect().unwrap();
                let request = task.take_producer_requests().pop().unwrap();
                let value = rt.root(Primitive::Null);
                task = rt
                    .complete(request.key(), request.producer(), Completion::Ready(&value))
                    .unwrap()
                    .into_runnable()
                    .pop()
                    .unwrap();
            }
            RunOutcome::Complete(value) => {
                assert_eq!(value.view(), ValueView::Bool(true));
                assert_eq!(waited, 2);
                return;
            }
            other => panic!("{other:?}"),
        }
    }
    panic!("registry key conversion did not finish");
}

/// Synthetic fixtures, not light-cljs output: throwing preserves primitive
/// identity and reports Symbols without performing implicit ToString.
#[test]
fn symbol_thrown_values() {
    for (expression, text) in [("Symbol()", "Symbol()"), ("Symbol('boom')", "Symbol(boom)")] {
        let source = format!(
            "let s = {expression}; let first = false; let caught = false; try {{ try {{ throw s; }} catch (e) {{ first = e === s; throw e; }} }} catch (e) {{ caught = e === s; }} first && caught"
        );
        assert_eq!(eval(&source), ValueView::Bool(true), "{source}");
        assert_eq!(
            failure_at(&format!("\n  throw {expression};")),
            (
                VmErrorKind::Thrown,
                format!("uncaught exception: {text}"),
                2,
                3
            )
        );
    }
}

/// Synthetic fixture, not light-cljs output: description callbacks and
/// symbol-keyed accessors retain their operands across suspension and GC.
#[test]
fn symbol_callbacks_wait_yield_collect_and_migrate() {
    let rt = Runtime::new();
    let source = r"
        let log = '';
        let s = Symbol({get toString() {
            log += 'g'; __rt.need(1271, 1);
            return function() { log += 's'; __rt.need(1272, 1); return 'key'; };
        }});
        let o = {};
        Object.defineProperty(o, s, {get() { __rt.need(1273, 1); return s; }});
        o[s] === s && String(s) === 'Symbol(key)' && log === 'gs'
    ";
    let mut task = rt.spawn(&exe(source), &[]).unwrap();
    let mut waited = 0;
    for _ in 0..1000 {
        let (next, outcome) = std::thread::spawn(move || {
            let outcome = task
                .run_with(Budget::instructions(1), &mut NoEffects)
                .unwrap();
            (task, outcome)
        })
        .join()
        .unwrap();
        task = next;
        match outcome {
            RunOutcome::Yielded => {
                rt.collect().unwrap();
            }
            RunOutcome::Waiting(_) => {
                waited += 1;
                assert!(waited <= 3);
                rt.collect().unwrap();
                let request = task.take_producer_requests().pop().unwrap();
                let value = rt.root(Primitive::Null);
                task = rt
                    .complete(request.key(), request.producer(), Completion::Ready(&value))
                    .unwrap()
                    .into_runnable()
                    .pop()
                    .unwrap();
            }
            RunOutcome::Complete(value) => {
                assert_eq!(value.view(), ValueView::Bool(true));
                assert_eq!(waited, 3);
                return;
            }
            other => panic!("{other:?}"),
        }
    }
    panic!("symbol callback did not finish");
}

/// Synthetic fixtures, not light-cljs output: numeric operators finish
/// ToNumber on the left before invoking conversion on the right.
#[test]
fn symbol_numeric_coercion_stops_before_rhs() {
    for op in ["-", "*", "/", "%", "**", "<<", ">>", ">>>", "&", "|", "^"] {
        for (left, expected) in [("s", ""), ("({valueOf() { log += 'l'; return s; }})", "l")] {
            for right in [
                "({valueOf() { log += 'r'; return 1; }})",
                "({valueOf() { throw 42; }})",
                "({valueOf() { __rt.need(1274, 1); return 1; }})",
            ] {
                let expression = format!("{left} {op} {right}");
                let source = format!(
                    "let s = Symbol(); let log = ''; let caught = false; try {{ {expression}; }} catch (e) {{ caught = e instanceof TypeError && e instanceof Error; }} caught && log === '{expected}'"
                );
                assert_eq!(eval(&source), ValueView::Bool(true), "{source}");
                let uncaught = format!("let s = Symbol(); let log = ''; {expression}");
                assert_eq!(
                    failure(&uncaught).kind(),
                    VmErrorKind::TypeError,
                    "{uncaught}"
                );
            }
        }
    }
    // Plus and relational operators first convert both objects to primitives.
    for expression in ["s + rhs", "s < rhs", "s > rhs", "s <= rhs", "s >= rhs"] {
        let source = format!(
            "let log = ''; let s = Symbol(); let rhs = {{valueOf() {{ log += 'r'; return 1; }}}}; let caught = false; try {{ {expression}; }} catch (e) {{ caught = e instanceof TypeError && e instanceof Error; }} caught && log === 'r'"
        );
        assert_eq!(eval(&source), ValueView::Bool(true), "{source}");
    }
}

/// Synthetic fixture, not light-cljs output: an inherited symbol setter
/// keeps its receiver and argument through VM waits, GC and migration.
#[test]
fn symbol_setters_wait_yield_collect_and_migrate() {
    let rt = Runtime::new();
    let source = r"
        let key = Symbol('setter');
        let proto = {};
        let receiver = Object.create(proto);
        let value = {payload: 7};
        let sawThis = false;
        let sawValue = false;
        let calls = 0;
        Object.defineProperty(proto, key, {set(v) {
            calls++;
            __rt.need(1275, 1);
            sawThis = this === receiver;
            __rt.need(1276, 1);
            sawValue = v === value;
            this.saved = v.payload;
        }});
        let result = (receiver[key] = value);
        result === value && sawThis && sawValue && calls === 1 && receiver.saved === 7
            && Object.getOwnPropertyDescriptor(receiver, key) === undefined
            && Object.keys(receiver).join() === 'saved'
    ";
    let mut task = rt.spawn(&exe(source), &[]).unwrap();
    let mut waited = 0;
    for _ in 0..1000 {
        let (next, outcome) = std::thread::spawn(move || {
            let outcome = task
                .run_with(Budget::instructions(1), &mut NoEffects)
                .unwrap();
            (task, outcome)
        })
        .join()
        .unwrap();
        task = next;
        match outcome {
            RunOutcome::Yielded => {
                rt.collect().unwrap();
            }
            RunOutcome::Waiting(_) => {
                waited += 1;
                assert!(waited <= 2);
                rt.collect().unwrap();
                let request = task.take_producer_requests().pop().unwrap();
                let value = rt.root(Primitive::Null);
                task = rt
                    .complete(request.key(), request.producer(), Completion::Ready(&value))
                    .unwrap()
                    .into_runnable()
                    .pop()
                    .unwrap();
            }
            RunOutcome::Complete(value) => {
                assert_eq!(value.view(), ValueView::Bool(true));
                assert_eq!(waited, 2);
                return;
            }
            other => panic!("{other:?}"),
        }
    }
    panic!("symbol setter did not finish");
}
/// Synthetic fixtures, not light-cljs output. The round trip adapts
/// QuickJS test_builtin.js::test_json at 7d1c13c5d8f8; the full MIT notice
/// is retained above at string_code_unit_access.
#[test]
fn json_stringify() {
    for src in [
        r#"let s = '{"x":1,"y":true,"z":null,"a":[1,2,3],"s":"str"}'; JSON.stringify(JSON.parse(s)) === s"#,
        r#"JSON.stringify(1) === '1' && JSON.stringify(-0) === '0' && JSON.stringify(NaN) === 'null' && JSON.stringify(Infinity) === 'null'"#,
        r#"JSON.stringify(true) === 'true' && JSON.stringify(false) === 'false' && JSON.stringify(null) === 'null'"#,
        r#"JSON.stringify('a"b\n\u0001') === '"a\\"b\\n\\u0001"'"#,
        r#"JSON.stringify('\ud800') === '"\\ud800"' && JSON.stringify('\udc00') === '"\\udc00"' && JSON.stringify('😀') === '"😀"'"#,
        r#"JSON.stringify('\b\f\r\t\\\u0000\u001f') === '"\\b\\f\\r\\t\\\\\\u0000\\u001f"'"#,
        r#"JSON.stringify([undefined, function () {}, NaN, , 1]) === '[null,null,null,null,1]'"#,
        r#"JSON.stringify({a: undefined, b: 1, c: function(){}}) === '{"b":1}'"#,
        r#"JSON.stringify(undefined) === undefined && JSON.stringify(function(){}) === undefined && JSON.stringify() === undefined"#,
        r#"JSON.stringify({}) === '{}' && JSON.stringify([]) === '[]'"#,
        r#"JSON.stringify({b: 1, a: 2, 1: 3}) === '{"1":3,"b":1,"a":2}'"#,
        r#"let o = {a: 1}; Object.defineProperty(o, 'b', {value: 2}); JSON.stringify(o) === '{"a":1}'"#,
        r#"JSON.stringify(new Number(3)) === '3' && JSON.stringify(new String('hi')) === '"hi"'"#,
        r#"JSON.stringify({toJSON: function () { return 5; }}) === '5'"#,
        r#"let log = ''; let o = {get toJSON() { log += 'g'; return function(k) { log += k; return this.x; }; }, x: 4}; JSON.stringify({a:o}) === '{"a":4}' && log === 'ga'"#,
        r#"let o = {toJSON(k) { return k; }}; JSON.stringify([o]) === '["0"]' && JSON.stringify(o) === '""'"#,
        r#"let f = function(){}; f.toJSON = function(){ return 7; }; JSON.stringify(f) === '7'"#,
        r#"JSON.stringify({toJSON(){ return {toJSON(){return 3;}, x:1}; }}) === '{"x":1}'"#,
        r#"let o = {valueOf() { return 6; }}; let n = new Number(3); n.valueOf = o.valueOf; JSON.stringify(n) === '6'"#,
        r#"let s = new String('x'); s.toString = function(){return 'y';}; JSON.stringify(s) === '"y"'"#,
        r#"let c = {}; c.self = c; let caught = false; try { JSON.stringify(c); } catch(e) { caught = true; } caught"#,
        r#"let c = []; c[0] = c; let caught = false; try { JSON.stringify(c); } catch(e) { caught = true; } caught"#,
        r#"let c = {x:1}; JSON.stringify([c,c]) === '[{"x":1},{"x":1}]'"#,
        r#"let c = {}; c.self = c; c.toJSON = function(){return 2;}; JSON.stringify(c) === '2'"#,
        r#"let o = {a:1, get b(){delete this.c; this.d = 4; return 2;}, c:3}; JSON.stringify(o) === '{"a":1,"b":2}'"#,
        r#"let a = [1,2]; Object.defineProperty(a, '0', {get(){this.length = 1; return 3;}}); JSON.stringify(a) === '[3,null]'"#,
        r#"let o = Object.create({a:3}); o.b = 2; JSON.stringify(o) === '{"b":2}'"#,
        r#"let d = Object.getOwnPropertyDescriptor(JSON, 'stringify'); d.writable && !d.enumerable && d.configurable && JSON.stringify.name === 'stringify' && JSON.stringify.length === 3"#,
        r#"JSON.stringify.call(null, 3) === '3' && JSON.stringify.apply(null, [4]) === '4' && JSON.stringify.bind(null, 5)() === '5'"#,
        r#"let caught = false; try { JSON.stringify({get a(){throw 42;}}); } catch(e) { caught = e === 42; } caught"#,
        r#"let caught = false; try { JSON.stringify({get toJSON(){throw 43;}}); } catch(e) { caught = e === 43; } caught"#,
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
    for src in [
        "JSON.stringify(1, function(k,v){return v;})",
        "JSON.stringify(1, null)",
        "JSON.stringify(1, [])",
    ] {
        let error = failure(src);
        assert_eq!(error.kind(), VmErrorKind::TypeError);
        assert!(
            error
                .message()
                .contains("JSON.stringify replacer is not supported")
        );
    }
    let error = failure("JSON.stringify(1, undefined, 2)");
    assert_eq!(error.kind(), VmErrorKind::TypeError);
    assert!(
        error
            .message()
            .contains("JSON.stringify space is not supported")
    );
    assert_eq!(
        failure("new JSON.stringify(1)").kind(),
        VmErrorKind::TypeError
    );
    assert_eq!(
        failure("let c = {}; c.self = c; JSON.stringify(c)").kind(),
        VmErrorKind::TypeError
    );
}

/// Synthetic fixture, not light-cljs output. Every callback suspends once;
/// continuation roots and active-path identities survive GC and migration.
#[test]
fn json_stringify_callbacks_wait_collect_and_migrate() {
    let rt = Runtime::new();
    let source = r#"
        let log = '';
        let result = JSON.stringify({keep: {n: 7}, get a() {
            log += 'a'; __rt.need(1, 1);
            return {get toJSON() {
                log += 'g'; __rt.need(2, 1);
                return function(key) {
                    log += key; __rt.need(3, 1);
                    let n = new Number(3);
                    n.valueOf = function() { log += 'v'; __rt.need(4, 1); return 8; };
                    return n;
                };
            }};
        }});
        result === '{"keep":{"n":7},"a":8}' && log === 'agav'
    "#;
    let mut task = rt.spawn(&exe(source), &[]).unwrap();
    let mut waited = 0;
    for _ in 0..1500 {
        let (next, outcome) = std::thread::spawn(move || {
            let outcome = task
                .run_with(Budget::instructions(1), &mut NoEffects)
                .unwrap();
            (task, outcome)
        })
        .join()
        .unwrap();
        task = next;
        match outcome {
            RunOutcome::Yielded => {
                rt.collect().unwrap();
            }
            RunOutcome::Waiting(_) => {
                waited += 1;
                assert!(waited <= 4);
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
            RunOutcome::Complete(value) => {
                assert_eq!(value.view(), ValueView::Bool(true));
                assert_eq!(waited, 4);
                return;
            }
            other => panic!("{other:?}"),
        }
    }
    panic!("JSON.stringify did not finish");
}

/// Synthetic fixtures, not light-cljs output. Isolate quoting from traversal
/// so a long string must yield independently of the number of members.
#[test]
fn json_stringify_long_strings_are_budgeted() {
    let rt = Runtime::new();
    let code =
        compile_function("function stringify(text) { return JSON.stringify(text); }").unwrap();
    for (unit, expected) in [(0xd800, "\\ud800"), (0x61, "a"), (0x0, "\\u0000")] {
        let root = rt.root(Primitive::String(
            clj_quickjs::JsString::from_code_units(vec![unit; 65536]).unwrap(),
        ));
        let task = rt.spawn(&code, &[&root]).unwrap();
        drop(root);
        let mut yields = 0;
        let mut complete = false;
        for _ in 0..10000 {
            match task
                .run_with(Budget::instructions(1), &mut NoEffects)
                .unwrap()
            {
                RunOutcome::Yielded => {
                    yields += 1;
                    rt.collect().unwrap();
                }
                RunOutcome::Complete(value) => {
                    assert_eq!(
                        value.string().unwrap(),
                        format!("\"{}\"", expected.repeat(65536)).as_str()
                    );
                    assert!(yields >= 256, "quoting must consume budget");
                    complete = true;
                    break;
                }
                other => panic!("{other:?}"),
            }
        }
        assert!(complete, "JSON.stringify did not finish");
    }
}

/// Synthetic fixtures, not light-cljs output. Deep input is constructed by a
/// loop, avoiding source/parser recursion; wide arrays stream their indices.
#[test]
fn json_stringify_depth_width_and_member_limits() {
    assert_eq!(
        eval(
            "let a = 7; for (let i = 0; i < 512; i++) { a = [a]; } JSON.stringify(a).length === 1025"
        ),
        ValueView::Bool(true)
    );
    let error = failure("let a = 7; for (let i = 0; i < 513; i++) { a = [a]; } JSON.stringify(a)");
    assert_eq!(error.kind(), VmErrorKind::RangeError);
    assert!(error.message().contains("nesting limit of 512"));
    assert_eq!(
        eval(
            "let caught = false; let a = 7; for (let i = 0; i < 10000; i++) { a = [a]; } try { JSON.stringify(a); } catch(e) { caught = true; } caught"
        ),
        ValueView::Bool(true)
    );
    assert_eq!(
        eval(
            "let a = []; for (let i = 0; i < 4097; i++) { a.push(1); } JSON.stringify(a).length === 8195"
        ),
        ValueView::Bool(true)
    );
    let code = compile_function(
        "function stringify(name) { let o = {}; o[name] = 2; return JSON.stringify(o); }",
    )
    .unwrap();
    let name = "a".repeat(4096);
    let RunOutcome::Complete(value) = run(&code, &[Primitive::String(name.as_str().into())]) else {
        panic!("bounded member name");
    };
    assert_eq!(value.string().unwrap().len(), 4102);
    let name = "a".repeat(4097);
    assert!(
        matches!(run(&code, &[Primitive::String(name.as_str().into())]), RunOutcome::Failed(e) if e.kind() == VmErrorKind::RangeError && e.message().contains("member name limit of 4096"))
    );
    assert_eq!(
        eval(
            "let o = {}; for (let i = 0; i < 4096; i++) { o[i] = 1; } JSON.parse(JSON.stringify(o))[4095] === 1"
        ),
        ValueView::Bool(true)
    );
    let error =
        failure("let o = {}; for (let i = 0; i < 4097; i++) { o[i] = 1; } JSON.stringify(o)");
    assert_eq!(error.kind(), VmErrorKind::RangeError);
    assert!(error.message().contains("object property limit of 4096"));
    for hidden in [false, true] {
        let property = if hidden {
            "Object.defineProperty(o, i, {value: 1})"
        } else {
            "o[i] = 1"
        };
        let check = if hidden {
            "JSON.stringify(o) === '{}'"
        } else {
            "JSON.parse(JSON.stringify(o))[4095] === 1"
        };
        assert_eq!(
            eval(&format!(
                "let o = new ArrayBuffer(0); for (let i = 0; i < 4096; i++) {{ {property}; }} {check}"
            )),
            ValueView::Bool(true)
        );
        let error = failure(&format!(
            "let o = new ArrayBuffer(0); for (let i = 0; i < 4097; i++) {{ {property}; }} JSON.stringify(o)"
        ));
        assert_eq!(error.kind(), VmErrorKind::RangeError);
        assert!(error.message().contains("object property limit of 4096"));
    }
}

/// Synthetic fixtures, not light-cljs output. toJSON results retain the
/// ordinary task ownership boundary, even for an empty foreign array.
#[test]
fn json_stringify_rejects_foreign_objects() {
    let rt = Runtime::new();
    for source in ["[]", "({})", "new Number(3)"] {
        let owner = rt.spawn(&exe(source), &[]).unwrap();
        let RunOutcome::Complete(value) = owner.run().unwrap() else {
            panic!("object");
        };
        for body in [
            "return JSON.stringify(value);",
            "return JSON.stringify({toJSON(){return value;}});",
        ] {
            let code =
                compile_function(&format!("function stringify(value) {{ {body} }}")).unwrap();
            let task = rt.spawn(&code, &[&value]).unwrap();
            assert!(
                matches!(task.run().unwrap(), RunOutcome::Failed(e) if matches!(e.kind(), VmErrorKind::ForeignObject { .. })),
                "{source} {body}"
            );
        }
    }
}

/// Synthetic fixtures, not light-cljs output. StringToNumber is a bounded
/// leaf after resumable Number-wrapper ToPrimitive, including fallback to
/// toString. Oversized callback results fail before any input-sized scan.
#[test]
fn json_stringify_number_wrapper_conversion_is_budgeted() {
    for fallback in [false, true] {
        let methods = if fallback {
            "n.valueOf = function(){calls += 1; return {};}; n.toString = function(){calls += 1; return text;};"
        } else {
            "n.valueOf = function(){calls += 1; return text;};"
        };
        let code = compile_function(&format!(
            "function stringify(text) {{ let n = new Number(3); let calls = 0; {methods} let result = JSON.stringify(n); return result === '7' && calls === {}; }}",
            if fallback { 2 } else { 1 }
        )).unwrap();
        let text = format!("{}7", "0".repeat(4095));
        let rt = Runtime::new();
        let root = rt.root(Primitive::String(text.as_str().into()));
        let task = rt.spawn(&code, &[&root]).unwrap();
        drop(root);
        let mut complete = false;
        let mut yields = 0;
        for _ in 0..1000 {
            match task
                .run_with(Budget::instructions(1), &mut NoEffects)
                .unwrap()
            {
                RunOutcome::Yielded => {
                    yields += 1;
                    rt.collect().unwrap();
                }
                RunOutcome::Complete(value) => {
                    assert_eq!(value.view(), ValueView::Bool(true));
                    assert!(yields > 10);
                    complete = true;
                    break;
                }
                other => panic!("{other:?}"),
            }
        }
        assert!(complete);
        let code = compile_function(&format!("function stringify(text) {{ let n = new Number(3); let calls = 0; {methods} return JSON.stringify(n); }}")).unwrap();
        for length in [4097, 65536] {
            let text = "1".repeat(length);
            let root = rt.root(Primitive::String(text.as_str().into()));
            let task = rt.spawn(&code, &[&root]).unwrap();
            drop(root);
            let mut failed = false;
            for _ in 0..1000 {
                match task
                    .run_with(Budget::instructions(1), &mut NoEffects)
                    .unwrap()
                {
                    RunOutcome::Yielded => {
                        rt.collect().unwrap();
                    }
                    RunOutcome::Failed(error) => {
                        assert_eq!(error.kind(), VmErrorKind::RangeError);
                        assert!(error.message().contains("Number conversion limit of 4096"));
                        failed = true;
                        break;
                    }
                    other => panic!("{other:?}"),
                }
            }
            assert!(failed);
        }
    }
}

/// Synthetic fixtures, not light-cljs output. Snapshot selection is own-only;
/// reads of selected keys and of toJSON follow the prototype chain.
#[test]
fn json_stringify_inherited_property_reads() {
    for source in [
        r#"let a = [,2]; Object.setPrototypeOf(a, {0:7}); JSON.stringify(a) === '[7,2]'"#,
        r#"let o = {get a(){delete this.b; return 1;}, b:2}; Object.setPrototypeOf(o, {b:3}); JSON.stringify(o) === '{"a":1,"b":3}'"#,
        r#"let p = {toJSON(k){return this.n + k;}}; let o = Object.create(p); o.n = 7; JSON.stringify({a:o}) === '{"a":"7a"}'"#,
        r#"let a = [,2]; let reads = 0; Object.setPrototypeOf(a, {get 0(){reads += 1; return 7;}}); JSON.stringify(a) === '[7,2]' && reads === 1"#,
    ] {
        assert_eq!(eval(source), ValueView::Bool(true), "{source}");
    }
}

/// Synthetic fixtures, not light-cljs output. Symbol values follow the same
/// omission rules as undefined; symbol-keyed getters are never read.
#[test]
fn json_stringify_symbols() {
    for source in [
        r#"JSON.stringify(Symbol()) === undefined && JSON.stringify([Symbol(), 1]) === '[null,1]' && JSON.stringify({a: Symbol(), b:1}) === '{"b":1}'"#,
        r#"let s = Symbol('key'); let o = {a:1}; o[s] = 2; Object.defineProperty(o, Symbol(), {get(){throw 42;}, enumerable:true}); JSON.stringify(o) === '{"a":1}'"#,
        r#"let o = {toJSON(){return Symbol();}}; JSON.stringify(o) === undefined && JSON.stringify([o]) === '[null]' && JSON.stringify({a:o}) === '{}'"#,
        r#"let n = new Number(3); n.valueOf = function(){return Symbol();}; let caught = false; try {JSON.stringify(n);} catch(e){caught = true;} caught"#,
    ] {
        assert_eq!(eval(source), ValueView::Bool(true), "{source}");
    }
}

/// Synthetic fixture, not light-cljs output. Integer and identity rows adapt
/// QuickJS tests/test_builtin.js::test_map and test_map1 (7d1c13c5d8f8).
/// Copyright (c) 2017-2021 Fabrice Bellard. MIT permission notice retained
/// in docs/attribution.md, "JavaScript Map tests".
#[test]
fn map_number_and_object_keys() {
    assert_eq!(failure("Map()").kind(), VmErrorKind::TypeError);
    for src in [
        "let m = new Map(); for (let i = 0; i < 1000; i++) m.set(i, i); let ok = m.size === 1000; for (let i = 0; i < 1000; i++) { if (m.get(i) !== i) ok = false; } m.set(-2147483648, 1); ok && m.get(-2147483648) === 1 && m.get(-2147483647 - 1) === 1 && m.get(-2147483647.5 - 0.5) === 1",
        "let m = new Map(); m.set(-0, 'z'); m.set(NaN, 1); m.get(0) === 'z' && m.get(NaN) === 1 && m.size === 2 && m.has(-0) && m.has(0 / 0)",
        "let m = new Map(); let keys = []; let values = []; for (let i = 0; i < 1000; i++) { let k = {id: i}; let v = {}; keys.push(k); values.push(v); m.set(k, v); } let ok = m.size === 1000; for (let i = 0; i < 1000; i++) { if (m.get(keys[i]) !== values[i]) ok = false; } ok && m.get({id: 0}) === undefined",
        "let m = new Map(); m.set(1, 2) === m && m.delete(1) === true && m.delete(1) === false && !m.has(1) && m.get(1) === undefined && m.size === 0",
        "let m = new Map(); m.set(1, 2); m.set(1, 3); let ok = m.size === 1 && m.get(1) === 3; m.set(2, 4); ok && m.clear() === undefined && m.size === 0 && !m.has(1) && !m.has(2) && m.set(1, 5) === m && m.size === 1 && m.get(1) === 5",
        "let m = new Map(); m.set(undefined, 1); m.set(null, 2); m.set(true, 3); m.set(false, 4); m.set('a' + 'b', 5); m.size === 5 && m.get() === 1 && m.get(null) === 2 && m.get(true) === 3 && m.get(false) === 4 && m.get('ab') === 5 && !m.has('missing')",
        "new Map(undefined).size === 0 && new Map(null).size === 0 && new Map().size === 0",
        "let caught = false; try { Map(); } catch (e) { caught = true; } caught",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
}

/// Synthetic fixtures, not light-cljs output. The 1000-entry deletion loop
/// adapts QuickJS tests/test_builtin.js::test_map1 (7d1c13c5d8f8).
/// Copyright (c) 2017-2021 Fabrice Bellard and Charlie Gordon; MIT permission
/// notice retained in docs/attribution.md, "JavaScript Map tests".
#[test]
fn map_for_each_live_iteration() {
    for src in [
        r#"
        let m = new Map(); let tab = [];
        for (let i = 0; i < 1000; i++) {
            let k = {id: i}; let v = {};
            tab.push([k, v]); m.set(k, v);
        }
        let i = 0; let ok = true; let receiver = {};
        let result = m.forEach(function(v, k, map) {
            if (k !== tab[i][0] || v !== tab[i][1] || map !== m ||
                this !== receiver || !m.has(k)) ok = false;
            i++;
            if (!m.delete(k) || m.has(k)) ok = false;
        }, receiver);
        ok && i === 1000 && m.size === 0 && result === undefined
        "#,
        "let m = new Map().set(1, 10).set(2, 20).set(3, 30); let seen = ''; m.forEach(function(v, k) { seen += k + ':' + v + ';'; if (k === 1) { m.delete(2); m.set(3, 31); m.set(4, 40); } }); seen === '1:10;3:31;4:40;'",
        "let m = new Map().set(1, 10).set(2, 20); let seen = ''; m.forEach(function(v, k) { seen += k; if (v === 10) { m.delete(1); m.set(1, 11); } }); seen === '121'",
        "let m = new Map().set(1, 10).set(2, 20); let seen = ''; m.forEach(function(v, k) { seen += k; m.clear(); }); seen === '1' && m.size === 0",
        "let m = new Map().set(1, 10).set(2, 20); let seen = ''; m.forEach(function(v, k) { seen += k; if (k === 1) { m.clear(); m.set(3, 30); } }); seen === '13'",
        "let m = new Map().set(1, 10).set(2, 20); let seen = ''; m.forEach(function(v, k) { m.forEach(function(v, inner) { seen += k + ':' + inner + ';'; }); }); seen === '1:1;1:2;2:1;2:2;'",
        "let m = new Map().set(1, 10).set(2, 20); let seen = 0; let caught = false; try { m.forEach(function() { seen++; throw 7; }); } catch(e) { caught = e === 7; } caught && seen === 1 && m.size === 2",
        "let caught = false; try { new Map().forEach(1); } catch(e) { caught = e instanceof TypeError; } caught",
        "let caught = false; try { new Map().forEach(); } catch(e) { caught = e instanceof TypeError; } caught",
        "let m = new Map().set(1, 10); let ok = false; m.forEach(function() { 'use strict'; ok = this === undefined; }); ok && new Map().forEach(function() { throw 1; }) === undefined",
        "let d = Object.getOwnPropertyDescriptor(Map.prototype, 'forEach'); d.writable && !d.enumerable && d.configurable && d.value.name === 'forEach' && d.value.length === 1",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
    for receiver in [
        "undefined",
        "null",
        "1",
        "{}",
        "Map.prototype",
        "Object.create(Map.prototype)",
    ] {
        let src = format!("Map.prototype.forEach.call({receiver}, function() {{}})");
        assert_eq!(failure(&src).kind(), VmErrorKind::TypeError, "{src}");
    }
    assert_eq!(
        failure("new Map.prototype.forEach()").kind(),
        VmErrorKind::TypeError
    );
}

/// Synthetic fixtures, not light-cljs output: brand checks and the ordinary
/// property store must remain independent of the entry storage.
#[test]
fn map_receivers_properties_and_explicit_gaps() {
    for src in [
        "let m = new Map(); m instanceof Map && Object.getPrototypeOf(m) === Map.prototype && m.constructor === Map && Map.name === 'Map' && Map.length === 1 && typeof m === 'object'",
        "let d = Object.getOwnPropertyDescriptor(Map, 'prototype'); d.value === Map.prototype && !d.writable && !d.enumerable && !d.configurable",
        "let d = Object.getOwnPropertyDescriptor(Map.prototype, 'size'); typeof d.get === 'function' && d.set === undefined && !d.enumerable && d.configurable && d.get.name === 'get size' && d.get.length === 0",
        "let m = new Map(); m.foo = 7; m[1] = 8; m.size === 0 && m.foo === 7 && m[1] === 8 && 'size' in m && Object.keys(m).length === 2 && delete m.foo && !('foo' in m)",
        "let m = new Map(); Object.defineProperty(m, 'foo', {value: 9}); m.foo === 9 && Object.getOwnPropertyDescriptor(m, 'foo').value === 9 && Object.getOwnPropertyDescriptor(m, 'size') === undefined",
        "let m = new Map(); Object.freeze(m); m.set(1, 2); Object.isFrozen(m) && !Object.isExtensible(m) && m.get(1) === 2 && m.size === 1",
        "let m = new Map(); let p = {x: 4}; Object.setPrototypeOf(m, p); m.x === 4 && Object.getPrototypeOf(m) === p && Map.prototype.set.call(m, 1, 2) === m && Map.prototype.get.call(m, 1) === 2",
        "let m = new Map(); let o = {get valueOf() { throw 1; }, get toString() { throw 2; }}; m.set(o, 9); m.get(o) === 9 && m.has(o) && m.delete(o)",
        "let m = new Map(); m.set(); m.has(undefined) && m.size === 1 && m.get() === undefined && m.delete() && !m.has()",
        "let m = new Map(); m.set(0, 'a'); m.set(-0, 'b'); m.set(NaN, 1); m.set(0/0, 2); m.size === 2 && m.get(0) === 'b' && m.get(NaN) === 2 && m.delete(-0) && !m.has(0)",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
    for (method, length) in [
        ("set", 2),
        ("get", 1),
        ("has", 1),
        ("delete", 1),
        ("clear", 0),
    ] {
        let src = format!(
            "let d = Object.getOwnPropertyDescriptor(Map.prototype, '{method}'); d.writable && !d.enumerable && d.configurable && d.value.name === '{method}' && d.value.length === {length}"
        );
        assert_eq!(eval(&src), ValueView::Bool(true), "{src}");
        for receiver in [
            "undefined",
            "null",
            "1",
            "'x'",
            "{}",
            "[]",
            "Map.prototype",
            "Object.create(Map.prototype)",
        ] {
            let src = format!("Map.prototype.{method}.call({receiver}, 1, 2)");
            assert_eq!(failure(&src).kind(), VmErrorKind::TypeError, "{src}");
        }
        assert_eq!(
            failure(&format!("new Map.prototype.{method}()")).kind(),
            VmErrorKind::TypeError
        );
    }
    for src in [
        "new Map([])",
        "new Map(1)",
        "new Map({})",
        "new Map('')",
        "Map.prototype.size",
        "Object.getOwnPropertyDescriptor(Map.prototype, 'size').get.call({})",
        "'use strict'; let m = new Map(); m.size = 2",
    ] {
        assert_eq!(failure(src).kind(), VmErrorKind::TypeError, "{src}");
        let caught =
            format!("let caught = false; try {{ {src}; }} catch (e) {{ caught = true; }} caught");
        assert_eq!(eval(&caught), ValueView::Bool(true), "{src}");
    }
}

/// Synthetic fixtures, not light-cljs output. BigInt rows adapt QuickJS
/// tests/test_builtin.js::test_map/test_map1 at 7d1c13c5d8f8.
/// Copyright (c) 2017-2021 Fabrice Bellard and Charlie Gordon.
/// MIT permission notice retained in docs/attribution.md, "JavaScript Map tests".
#[test]
fn map_bigint_keys() {
    for source in [
        "let m = new Map(); m.set(1n, 1n); m.get(1n) === 1n && m.get(2n**1000n - (2n**1000n - 1n)) === 1n",
        "let m = new Map(); m.set(1n, 'a'); m.set(1, 'b'); m.size === 2 && m.get(1n) === 'a' && m.get(1) === 'b'",
        "let m = new Map(); m.set(0n, 1); m.get(-0n) === 1 && m.get(-0n) === m.get(0n) && m.size === 1",
        // Separate limb allocations, signs and both sides of the inline boundary.
        "let m = new Map(); m.set(9223372036854775807n, 1); m.set(9223372036854775808n, 2); m.set(-9223372036854775808n, 3); m.set(-9223372036854775809n, 4); m.size === 4 && m.get((1n << 63n) - 1n) === 1 && m.get(1n << 63n) === 2 && m.get(-(1n << 63n)) === 3 && m.get(-(1n << 63n) - 1n) === 4",
        "let m = new Map(); m.set(0x100000000000000000000000000000001n, 'old'); m.set((1n << 128n) + 1n, 'new'); m.set(-((1n << 128n) + 1n), 'negative'); m.size === 2 && m.get(0x100000000000000000000000000000001n) === 'new' && m.get(-0x100000000000000000000000000000001n) === 'negative' && !m.has((1n << 128n) + 2n)",
        // The largest permitted magnitude still hashes by content in a leaf call.
        "let m = new Map(); m.set(1n << 1048575n, 7); m.get(2n << 1048574n) === 7 && m.has(2n << 1048574n) && m.delete(2n << 1048574n) && m.size === 0",
    ] {
        assert_eq!(eval(source), ValueView::Bool(true), "{source}");
    }
    for offset in ["0n", "1n << 128n"] {
        let source = format!(
            r#"
            let m = new Map(); let values = []; let offset = {offset};
            for (let i = 0; i < 1000; i++) {{
                let v = {{id: i}}; values.push(v); m.set(BigInt(i) + offset, v);
            }}
            let ok = m.size === 1000;
            for (let i = 0; i < 1000; i++) {{
                let key = BigInt(i + 1) + offset - 1n;
                if (m.get(key) !== values[i] || !m.has(key)) ok = false;
                m.set(key, values[i]);
            }}
            ok = ok && m.size === 1000;
            for (let i = 0; i < 1000; i++) {{
                let key = BigInt(i + 2) + offset - 2n;
                if (!m.has(key) || !m.delete(key) || m.has(key)) ok = false;
            }}
            ok && m.size === 0
            "#
        );
        assert_eq!(eval(&source), ValueView::Bool(true), "{offset}");
    }
}

/// Synthetic fixture, not light-cljs output; upstream attribution as above.
#[test]
fn map_bigint_for_each_deletes_all_keys() {
    for offset in ["0n", "1n << 128n"] {
        let source = format!(
            "let m = new Map(); let offset = {offset}; for (let i = 0; i < 1000; i++) m.set(BigInt(i) + offset, i); let i = 0; m.forEach(function(v, k) {{ if (k !== BigInt(i) + offset || v !== i || !m.has(k) || !m.delete(k) || m.has(k)) throw 1; i++; }}); i === 1000 && m.size === 0"
        );
        assert_eq!(eval(&source), ValueView::Bool(true), "{offset}");
    }
}

#[test]
fn maps_reject_access_from_another_task() {
    let rt = Runtime::new();
    for source in ["new Map()", "new Map().set(1, 2)"] {
        let owner = rt.spawn(&exe(source), &[]).unwrap();
        let RunOutcome::Complete(map) = owner.run().unwrap() else {
            panic!("expected Map")
        };
        for body in [
            "m.size",
            "m.get(1)",
            "m.set(1, 3)",
            "Map.prototype.get.call(m, 1)",
            // Borrow the method to bypass receiver-property access checks.
            // A callback throw must never replace the native ownership error,
            // and empty Maps must reject ownership before checking entries.
            "Map.prototype.forEach.call(m, function() { throw 170; })",
            "m.foo = 3",
            "Object.keys(m)",
            "Object.getPrototypeOf(m)",
            "Object.isFrozen(m)",
            "Object.freeze(m)",
            "Object.prototype.toString.call(m)",
        ] {
            let code = compile_function(&format!("function read(m) {{ return {body}; }}")).unwrap();
            let task = rt.spawn(&code, &[&map]).unwrap();
            assert!(
                matches!(task.run().unwrap(), RunOutcome::Failed(e) if matches!(e.kind(), VmErrorKind::ForeignObject { .. })),
                "{source}: {body}"
            );
        }
    }
}

/// Synthetic fixtures, not light-cljs output. Symbols retain primitive
/// identity and never coerce to strings while hashing or comparing keys.
#[test]
fn map_symbol_keys_and_tag() {
    for receiver in [
        "new Map()",
        "Map.prototype",
        "Object.create(Map.prototype)",
        "Object.create(Object.create(Map.prototype))",
    ] {
        let src = format!("Object.prototype.toString.call({receiver})");
        assert_eq!(
            failure(&src).kind(),
            VmErrorKind::UnsupportedOperand,
            "{src}"
        );
    }
    for src in [
        "let m = new Map(); Object.setPrototypeOf(m, null); Object.prototype.toString.call(m) === '[object Object]'",
        "let m = new Map(); let a = Symbol('key'); let b = Symbol('key'); m.set(a, 1); m.set(b, 2); m.set('Symbol(key)', 3); m.size === 3 && m.get(a) === 1 && m.get(b) === 2 && m.get('Symbol(key)') === 3 && !m.has(Symbol('key'))",
        "let m = new Map(); let s = Symbol(); m.set(s, 1); m.set(s, 2); m.size === 1 && m.get(s) === 2 && m.delete(s) && !m.delete(s) && !m.has(s)",
        "let m = new Map(); m.set(Symbol.iterator, 1); m.set(Symbol.toStringTag, 2); m.get(Symbol.iterator) === 1 && m.get(Symbol.toStringTag) === 2 && m.size === 2",
        "typeof Symbol.toStringTag === 'symbol' && Symbol.toStringTag !== Symbol('Symbol.toStringTag') && Map.prototype[Symbol.toStringTag] === 'Map' && new Map()[Symbol.toStringTag] === 'Map'",
        "let d = Object.getOwnPropertyDescriptor(Map.prototype, Symbol.toStringTag); d.value === 'Map' && !d.writable && !d.enumerable && d.configurable",
        "let d = Object.getOwnPropertyDescriptor(Symbol, 'toStringTag'); d.value === Symbol.toStringTag && !d.writable && !d.enumerable && !d.configurable",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
}

/// Synthetic fixtures, not light-cljs output. Deferred tag consumption
/// rejects presence without calling getters, even before a branded fallback.
#[test]
fn object_to_string_rejects_deferred_tags() {
    for receiver in ["{}", "[]", "function() {}", "new Error()", "new Number(1)"] {
        for descriptor in [
            "{ value: undefined }",
            "{ get: function() { throw new Error('tag getter ran'); } }",
        ] {
            let src = format!(
                "let o = {receiver}; Object.defineProperty(o, Symbol.toStringTag, {descriptor}); Object.prototype.toString.call(o)"
            );
            assert_eq!(
                failure(&src).kind(),
                VmErrorKind::UnsupportedOperand,
                "{src}"
            );
        }
    }
}

/// Synthetic fixtures, not light-cljs output. Tag probing follows the
/// same task ownership boundary as other property reads.
#[test]
fn object_to_string_rejects_foreign_receivers() {
    let rt = Runtime::new();
    let code =
        compile_function("function tag(value) { return Object.prototype.toString.call(value); }")
            .unwrap();
    for receiver in ["({})", "[]"] {
        let owner = rt.spawn(&exe(receiver), &[]).unwrap();
        let RunOutcome::Complete(value) = owner.run().unwrap() else {
            panic!("expected {receiver}")
        };
        let caller = rt.spawn(&code, &[&value]).unwrap();
        let RunOutcome::Failed(error) = caller.run().unwrap() else {
            panic!("expected foreign receiver failure for {receiver}")
        };
        assert_eq!(
            error.kind(),
            VmErrorKind::ForeignObject {
                owner: Some(owner.id()),
                caller: caller.id(),
            },
            "{receiver}"
        );
    }
}

/// Synthetic fixture, not light-cljs output.
#[test]
fn intl_namespace_object() {
    assert_eq!(
        eval(
            r#"
            let call = false; let construct = false;
            try { Intl(); } catch (e) { call = e instanceof TypeError; }
            try { new Intl(); } catch (e) { construct = e instanceof TypeError; }
            let d = Object.getOwnPropertyDescriptor(Intl, Symbol.toStringTag);
            typeof Intl === 'object' && Object.getPrototypeOf(Intl) === Object.prototype &&
            Object.prototype.toString.call(Intl) === '[object Intl]' && call && construct &&
            d.value === 'Intl' && !d.writable && !d.enumerable && d.configurable &&
            globalThis.Intl === Intl
        "#
        ),
        ValueView::Bool(true)
    );
}

/// Synthetic fixtures, not light-cljs output. The bigint_pow rows adapt
/// QuickJS tests/test_bigint.js at 7d1c13c5d8f8c6b32893786ed35cdcebd88194d2.
/// Copyright (c) 2017-2021 Fabrice Bellard
/// Copyright (c) 2017-2021 Charlie Gordon
///
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
/// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
/// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
/// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
/// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
/// THE SOFTWARE.
#[test]
fn bigint_additive_and_multiplicative_operators() {
    for product in ["r = r * a", "r *= a"] {
        let src = format!(
            "function bigint_pow(a, n) {{ let r = 1n; for (let i = 0n; i < n; i++) {{ {product}; }} return r; }}
             let r = bigint_pow(3n, 100n);
             r === 515377520732011331036461129765621272702107522001n && (r - 1n) !== r"
        );
        assert_eq!(eval(&src), ValueView::Bool(true), "{src}");
    }
    for src in [
        "-(-5n) === 5n && -0n === 0n && 2n * 3n - 1n === 5n",
        "let x = 0n; let old = x++; let next = ++x; let prev = x--; old === 0n && next === 2n && prev === 2n && x === 1n && --x === 0n",
        "let x = 9223372036854775807n; let old = x++; old === 9223372036854775807n && x === 9223372036854775808n && --x === old",
        "let x = -9223372036854775808n; let old = x--; old === -9223372036854775808n && x === -9223372036854775809n && ++x === old",
        "-(-9223372036854775808n) === 9223372036854775808n && -9223372036854775808n + 9223372036854775808n === 0n",
        "let x = 5n; x += 3n; x -= 2n; x *= -4n; x === -24n",
        "let o = {x: 0n}; let old = o.x++; let next = ++o['x']; o.x *= 3n; o['x'] -= 1n; old === 0n && next === 2n && o.x === 5n",
        "let x = 1n; function f() { return x++; } f() === 1n && f() === 2n && x === 3n",
        "(1n - 1n ? false : true) && typeof (2n * 3n) === 'bigint' && typeof -0n === 'bigint'",
        "-1n + -2n === -3n && -2n < -1n && 2n * 3n < 7n && (!1 + 1) === 1",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
    let src = format!(
        "{n}n * {n}n - {square}n === 0n",
        n = "9".repeat(300),
        square = format_args!("{}8{}1", "9".repeat(299), "0".repeat(299)),
    );
    assert_eq!(eval(&src), ValueView::Bool(true));
    for src in [
        "1n + 1",
        "1 + 1n",
        "1 - 1n",
        "1n - 1",
        "2n * 2",
        "2 * 2n",
        "1n + true",
        "true + 1n",
        "1n + undefined",
        "undefined + 1n",
        "1n + null",
        "null + 1n",
        "1n - false",
        "null * 1n",
        "+1n",
        "+(-1n)",
        "+(2n * 3n)",
        "1 + -2n",
    ] {
        let caught = format!(
            "let caught = false; try {{ {src}; }} catch (e) {{ caught = e instanceof TypeError; }} caught"
        );
        assert_eq!(eval(&caught), ValueView::Bool(true), "{src}");
    }
}

/// Synthetic fixture, not light-cljs output. A failed intermediate operation
/// must throw even when a later operation would cancel it back under the cap.
#[test]
fn bigint_arithmetic_size_errors_are_catchable_and_leave_bindings_unchanged() {
    let rt = Runtime::new();
    let cap = clj_quickjs::JsBigInt::from_sign_magnitude(
        false,
        vec![u32::MAX; clj_quickjs::MAX_BIGINT_BITS / 32],
    )
    .unwrap();
    let root = rt.root(Primitive::BigInt(cap));
    for expression in [
        "a + 1n",
        "-a - 1n",
        "a * 2n",
        "a++",
        "++a",
        "a += 1n",
        "a *= 2n",
        "(-a - 1n) + 1n",
        "(a + 1n) - 1n",
        "(a * 2n) * 0n",
    ] {
        let src = format!(
            "function f(a) {{ let original = a; let caught = false; try {{ {expression}; }} catch (e) {{ caught = e instanceof RangeError; }} return caught && a === original; }}"
        );
        let f = compile_function(&src).unwrap();
        assert!(
            matches!(rt.spawn(&f, &[&root]).unwrap().run().unwrap(), RunOutcome::Complete(v) if v.view() == ValueView::Bool(true)),
            "{expression}"
        );
    }
}

/// Synthetic fixtures, not light-cljs output. Conversion callbacks run once,
/// in source order, and retain their BigInt values over waits/yields/collection.
#[test]
fn bigint_arithmetic_resumes_object_conversion() {
    for expression in [
        "a + b === 11n",
        "a - b === 3n",
        "a * b === 28n",
        "a / b === 1n",
        "a % b === 3n",
        "a ** b === 2401n",
        "(a & b) === 4n",
        "(a | b) === 7n",
        "(a ^ b) === 3n",
        "a << b === 112n",
        "a >> b === 0n",
        "~a === -8n",
        "-a === -7n",
        "a++ === 7n && a === 8n",
        "++a === 8n",
    ] {
        let rt = Runtime::new();
        let both = expression.contains('b');
        let src = format!(
            "let log = ''; let a = {{ valueOf() {{ log += 'a'; __rt.need(1981, 1); return 7n; }} }}; let b = {{ valueOf() {{ log += 'b'; __rt.need(1982, 1); return 4n; }} }}; ({expression}) && log === '{}'",
            if both { "ab" } else { "a" }
        );
        let mut task = rt.spawn(&exe(&src), &[]).unwrap();
        let mut waits = 0;
        let mut completed = false;
        for _ in 0..1000 {
            let (next, outcome) = std::thread::spawn(move || {
                let outcome = task
                    .run_with(Budget::instructions(1), &mut NoEffects)
                    .unwrap();
                (task, outcome)
            })
            .join()
            .unwrap();
            task = next;
            match outcome {
                RunOutcome::Yielded => {
                    rt.collect().unwrap();
                }
                RunOutcome::Waiting(_) => {
                    waits += 1;
                    assert!(waits <= if both { 2 } else { 1 });
                    rt.collect().unwrap();
                    let request = task.take_producer_requests().pop().unwrap();
                    let value = rt.root(Primitive::Null);
                    task = rt
                        .complete(request.key(), request.producer(), Completion::Ready(&value))
                        .unwrap()
                        .into_runnable()
                        .pop()
                        .unwrap();
                }
                RunOutcome::Complete(v) => {
                    assert_eq!(v.view(), ValueView::Bool(true), "{expression}");
                    assert_eq!(waits, if both { 2 } else { 1 });
                    completed = true;
                    break;
                }
                other => panic!("{expression}: {other:?}"),
            }
        }
        assert!(completed, "{expression}");
    }
}

/// Synthetic fixtures, not light-cljs output. The update completes before
/// unary plus rejects its BigInt result; comparisons preserve its numeric type.
#[test]
fn bigint_update_results_use_numeric_coercion() {
    for (setup, target) in [
        ("let x = 2n;", "x"),
        ("let o = {x: 2n};", "o.x"),
        ("let o = {x: 2n};", "o['x']"),
    ] {
        for (expression, updated, less) in [
            (format!("{target}++"), 3, true),
            (format!("++{target}"), 3, false),
            (format!("{target}--"), 1, true),
            (format!("--{target}"), 1, true),
        ] {
            let source = format!(
                "{setup} let caught = false; try {{ +({expression}); }} catch (e) {{ caught = e instanceof TypeError; }} caught && {target} === {updated}n"
            );
            assert_eq!(eval(&source), ValueView::Bool(true), "{source}");
            let source = format!(
                "{setup} let result = ({expression}) < 3; result === {less} && {target} === {updated}n"
            );
            assert_eq!(eval(&source), ValueView::Bool(true), "{source}");
        }
    }
}

/// Synthetic fixture, not light-cljs output.
#[test]
fn intl_locale_construct_and_canonicalize() {
    for tag in [
        "en", "de", "ja", "zh", "zh-Hant", "ru", "fr", "ar", "pt-BR", "en-GB",
    ] {
        assert_eq!(
            eval(&format!("new Intl.Locale('{tag}').toString() === '{tag}'")),
            ValueView::Bool(true)
        );
    }
    assert_eq!(
        eval(
            r#"
        let range = false; let type = false; let call = false;
        try { new Intl.Locale(''); } catch(e) { range = e instanceof RangeError; }
        try { new Intl.Locale(5); } catch(e) { type = e instanceof TypeError; }
        try { Intl.Locale('en'); } catch(e) { call = e instanceof TypeError; }
        new Intl.Locale('zh-Hant').script === 'Hant' &&
        new Intl.Locale('pt-BR').region === 'BR' &&
        new Intl.Locale('de-u-co-phonebk').collation === 'phonebk' &&
        new Intl.Locale('en', {region: 'GB'}).toString() === 'en-GB' &&
        new Intl.Locale('en').baseName === 'en' &&
        Intl.getCanonicalLocales(['EN-us', 'en-US']).join() === 'en-US' &&
        range && type && call
    "#
        ),
        ValueView::Bool(true)
    );
}

/// Adapted test_locale rows from QuickJS tests/test_intl.js, revision
/// 7d1c13c5d8f8c6b32893786ed35cdcebd88194d2. Synthetic fixture, not light-cljs output.
///
/// Copyright (c) 2017-2021 Fabrice Bellard
/// Copyright (c) 2017-2021 Charlie Gordon
///
/// Permission is hereby granted, free of charge, to any person obtaining a copy
/// of this software and associated documentation files (the "Software"), to deal
/// in the Software without restriction, including without limitation the rights
/// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
/// copies of the Software, and to permit persons to whom the Software is
/// furnished to do so, subject to the following conditions:
///
/// The above copyright notice and this permission notice shall be included in
/// all copies or substantial portions of the Software.
///
/// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
/// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
/// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL
/// THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
/// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
/// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
/// THE SOFTWARE.
#[test]
fn intl_locale_maximize_minimize() {
    for (method, rows) in [
        (
            "maximize",
            &[
                ("en", "en-Latn-US"),
                ("de", "de-Latn-DE"),
                ("ja", "ja-Jpan-JP"),
                ("zh", "zh-Hans-CN"),
                ("zh-Hant", "zh-Hant-TW"),
                ("ru", "ru-Cyrl-RU"),
                ("fr", "fr-Latn-FR"),
                ("ar", "ar-Arab-EG"),
                ("pt-BR", "pt-Latn-BR"),
            ][..],
        ),
        (
            "minimize",
            &[
                ("en", "en"),
                ("de", "de"),
                ("ja", "ja"),
                ("zh-Hant", "zh-TW"),
                ("en-GB", "en-GB"),
                ("en-Cyrl-GB", "en-Cyrl-GB"),
            ][..],
        ),
    ] {
        for (tag, expected) in rows {
            let source = format!("new Intl.Locale('{tag}').{method}().toString() === '{expected}'");
            assert_eq!(eval(&source), ValueView::Bool(true), "{source}");
        }
    }
    for source in [
        "new Intl.Locale('en-u-ca-gregory').maximize().toString() === 'en-Latn-US-u-ca-gregory'",
        "new Intl.Locale('zh-Hant-u-ca-gregory').minimize().toString() === 'zh-TW-u-ca-gregory'",
        "new Intl.Locale('und-Hant-hakka').maximize().toString() === 'hak-Hant-TW'",
        "new Intl.Locale('und-Hant-hakka').minimize().toString() === 'hak-TW'",
        "let l = new Intl.Locale('und-CN-hakka-a-foobar-u-ca-gregory-x-private'); let m = l.maximize(); let n = l.minimize(); m.toString() === 'hak-Hans-CN-a-foobar-u-ca-gregory-x-private' && n.toString() === 'hak-a-foobar-u-ca-gregory-x-private' && Intl.getCanonicalLocales([m,n]).join() === m.toString() + ',' + n.toString()",
        "let l = new Intl.Locale('en'); let m = l.maximize(); let n = l.minimize(); m !== l && n !== l && m !== n && l.toString() === 'en' && m instanceof Intl.Locale && Object.getPrototypeOf(m) === Intl.Locale.prototype",
        "let l = new Intl.Locale('de-1901-a-foobar-u-attr-ca-gregory-kn-x-private'); l.maximize().toString() === 'de-Latn-DE-1901-a-foobar-u-attr-ca-gregory-kn-x-private' && l.maximize().minimize().toString() === l.toString()",
        "let l = new Intl.Locale('en'); l.toString = function(){throw 9;}; l.maximize().toString() === 'en-Latn-US'",
    ] {
        assert_eq!(eval(source), ValueView::Bool(true), "{source}");
    }
    for method in ["maximize", "minimize"] {
        for receiver in [
            "{}",
            "null",
            "'en'",
            "Intl.Locale.prototype",
            "Object.create(Intl.Locale.prototype)",
        ] {
            let source = format!(
                "let caught = false; try {{ Intl.Locale.prototype.{method}.call({receiver}); }} catch(e) {{ caught = e instanceof TypeError; }} caught"
            );
            assert_eq!(eval(&source), ValueView::Bool(true), "{source}");
        }
        let source = format!(
            "let d = Object.getOwnPropertyDescriptor(Intl.Locale.prototype, '{method}'); d.value.name === '{method}' && d.value.length === 0 && d.writable && !d.enumerable && d.configurable"
        );
        assert_eq!(eval(&source), ValueView::Bool(true), "{source}");
        let source = format!(
            "let caught = false; try {{ new Intl.Locale.prototype.{method}(); }} catch(e) {{ caught = e instanceof TypeError; }} caught"
        );
        assert_eq!(eval(&source), ValueView::Bool(true), "{source}");
    }
}

/// Synthetic fixtures, not light-cljs output.
#[test]
fn intl_locale_options_lists_and_brand_checks() {
    for source in [
        "Intl.getCanonicalLocales(true).length === 0 && Intl.getCanonicalLocales(Symbol()).length === 0 && Intl.getCanonicalLocales(1n).length === 0",
        "new Intl.Locale('en-u-kn-yes').numeric === true && Intl.getCanonicalLocales(['en-u-kn-yes','en-u-kn']).join() === 'en-u-kn'",
        "let l = new Intl.Locale('en-u-ca-co-hc-kf-nu'); l.calendar === '' && l.collation === '' && l.hourCycle === '' && l.caseFirst === '' && l.numberingSystem === ''",
        r#"let l = new Intl.Locale('en-u-ca-true-co-true-hc-true-kf-true-nu-true'); l.toString() === 'en-u-ca-co-hc-kf-nu' && l.calendar === '' && l.collation === '' && l.hourCycle === '' && l.caseFirst === '' && l.numberingSystem === '' && l.numeric === false"#,
        r#"let l = new Intl.Locale('EN-latn-us-u-ca-buddhist-co-dict-hc-h12-kf-lower-kn-false-nu-arab-x-Private', {language:'de', script:'CYRL', region:'gb', calendar:'islamicc', collation:'phonebk', hourCycle:'h23', caseFirst:'upper', numeric: {}, numberingSystem:'LATN'}); l.toString() === 'de-Cyrl-GB-u-ca-islamic-civil-co-phonebk-hc-h23-kf-upper-kn-nu-latn-x-private' && l.baseName === 'de-Cyrl-GB' && l.language === 'de' && l.script === 'Cyrl' && l.region === 'GB' && l.calendar === 'islamic-civil' && l.collation === 'phonebk' && l.hourCycle === 'h23' && l.caseFirst === 'upper' && l.numeric === true && l.numberingSystem === 'latn'"#,
        r#"let a = new Intl.Locale('en-u-kn'); a.toString = function() { throw 9; }; new Intl.Locale(a, {numeric: false}).toString() === 'en-u-kn-false' && Intl.getCanonicalLocales([a, 'en-u-kn-true']).join() === 'en-u-kn' && Intl.getCanonicalLocales(a)[0] === 'en-u-kn'"#,
        r#"let l = new Intl.Locale('xx'); l.script === undefined && l.region === undefined && l.calendar === undefined && l.numeric === false && l[Symbol.toStringTag] === 'Intl.Locale' && Object.getPrototypeOf(l) === Intl.Locale.prototype && l instanceof Intl.Locale"#,
        r#"Intl.getCanonicalLocales().length === 0 && Intl.getCanonicalLocales(5).length === 0 && Intl.getCanonicalLocales('EN-us').join() === 'en-US' && Intl.getCanonicalLocales({0:'fr', 2:'EN-us', 3:'fr', length:4}).join() === 'fr,en-US' && Intl.getCanonicalLocales({length:-1}).length === 0"#,
        r#"let proto = {1:'de'}; let list = Object.create(proto); list[0] = 'fr'; list.length = 2; Intl.getCanonicalLocales(list).join() === 'fr,de'"#,
        r#"let log = ''; let l = new Intl.Locale({toString(){log += 't';return 'en';}}, {get language(){log += 'l';return {toString(){log += 's';return 'DE';}};}, get script(){log += 'S';}, get region(){log += 'r';return 'GB';}, get calendar(){log += 'c';}, get collation(){log += 'C';}, get hourCycle(){log += 'h';}, get caseFirst(){log += 'f';}, get numeric(){log += 'n';return {valueOf(){throw 9;}};}, get numberingSystem(){log += 'N';}}); l.toString() === 'de-GB-u-kn' && log === 'tlsSrcChfnN'"#,
        r#"let log = ''; let list = {get length(){log += 'l';return {valueOf(){log += 'n';return 3.9;}};}, get 0(){log += 'a';return {toString(){log += 's';return 'DE';}};}, get 2(){log += 'b';return 'FR';}}; Intl.getCanonicalLocales(list).join() === 'de,fr' && log === 'lnasb'"#,
        r#"let d = Object.getOwnPropertyDescriptor(Intl.Locale.prototype,'language'); let t = Object.getOwnPropertyDescriptor(Intl.Locale.prototype,Symbol.toStringTag); d.get.name === 'get language' && d.get.length === 0 && d.set === undefined && !d.enumerable && d.configurable && t.value === 'Intl.Locale' && !t.writable && !t.enumerable && t.configurable && Intl.Locale.name === 'Locale' && Intl.Locale.length === 1 && Intl.getCanonicalLocales.length === 1 && !Object.getOwnPropertyDescriptor(Intl,'Locale').enumerable"#,
        r#"let caught = false; let log = ''; try { Intl.getCanonicalLocales({length:2, get 0(){ log += 'a'; throw 42; }, get 1(){log += 'b'; return 'en';}}); } catch(e) {caught = e === 42;} caught && log === 'a'"#,
        r#"let caught = false; let log = ''; try { new Intl.Locale('en', {get language(){log += 'l';return 'bad!';}, get region(){log += 'r';return 'US';}}); } catch(e) { caught = e instanceof RangeError; } caught && log === 'l'"#,
    ] {
        assert_eq!(eval(source), ValueView::Bool(true), "{source}");
    }
    for source in [
        "Intl.Locale.prototype.toString.call({})",
        "Object.getOwnPropertyDescriptor(Intl.Locale.prototype,'language').get.call({})",
        "Intl.Locale.prototype.toString.call(Intl.Locale.prototype)",
        "Intl.getCanonicalLocales([5])",
        "Intl.getCanonicalLocales([null])",
        "Intl.getCanonicalLocales([undefined])",
        "Intl.getCanonicalLocales([Symbol()])",
        "Intl.getCanonicalLocales(null)",
        "new Intl.Locale()",
        "new Intl.Locale(Symbol())",
        "new Intl.Locale('en', null)",
        "new Intl.Locale('en', 5)",
        "new Intl.Locale('en', true)",
        "new Intl.Locale('en', 'options')",
        "new Intl.Locale('en', Symbol())",
        "new Intl.Locale('en', 1n)",
        "new Intl.Locale('', null)",
        "new Intl.Locale('', 5)",
        "new Intl.Locale('en', {region:Symbol()})",
        "new Intl.getCanonicalLocales('en')",
    ] {
        assert_eq!(failure(source).kind(), VmErrorKind::TypeError, "{source}");
    }
    for source in [
        "new Intl.Locale('en-', {region:'US'})",
        "new Intl.Locale('en', {language:'abcd'})",
        "new Intl.Locale('en', {script:'1234'})",
        "new Intl.Locale('en', {region:'USA'})",
        "new Intl.Locale('en', {calendar:'ab'})",
        "new Intl.Locale('en', {collation:'a-b'})",
        "new Intl.Locale('en', {hourCycle:'H12'})",
        "new Intl.Locale('en', {caseFirst:'UPPER'})",
        "new Intl.Locale('en', {numberingSystem:'l!'})",
        "Intl.getCanonicalLocales(['en', ''])",
    ] {
        assert_eq!(failure(source).kind(), VmErrorKind::RangeError, "{source}");
    }
}

/// Synthetic fixture, not light-cljs output. Each callback waits once and
/// survives single-instruction yields, collection and migration to fresh threads.
#[test]
fn intl_locale_callbacks_wait_collect_and_migrate() {
    let rt = Runtime::new();
    let source = r#"
        let log = '';
        let locale = new Intl.Locale({toString(){log += 't'; __rt.need(1,1); return 'en-u-ca-gregory';}}, {
            get region(){log += 'r'; __rt.need(2,1); return {toString(){log += 's'; __rt.need(3,1); return 'GB';}};},
            get numeric(){log += 'n'; __rt.need(4,1); return true;}
        });
        let locales = Intl.getCanonicalLocales({get length(){log += 'l'; __rt.need(5,1); return {valueOf(){log += 'v'; __rt.need(6,1); return 2;}};},
            get 0(){log += 'a'; __rt.need(7,1); return {toString(){log += 'S'; __rt.need(8,1); return 'DE';}};},
            get 1(){log += 'b'; __rt.need(9,1); return locale;}
        });
        locales.join() === 'de,en-GB-u-ca-gregory-kn' && log === 'trsnlvaSb' && locale.region === 'GB'
    "#;
    let mut task = rt.spawn(&exe(source), &[]).unwrap();
    let mut waited = 0;
    for _ in 0..2000 {
        let (next, outcome) = std::thread::spawn(move || {
            let outcome = task
                .run_with(Budget::instructions(1), &mut NoEffects)
                .unwrap();
            (task, outcome)
        })
        .join()
        .unwrap();
        task = next;
        match outcome {
            RunOutcome::Yielded => {
                rt.collect().unwrap();
            }
            RunOutcome::Waiting(_) => {
                waited += 1;
                assert!(waited <= 9);
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
            RunOutcome::Complete(value) => {
                assert_eq!(value.view(), ValueView::Bool(true));
                assert_eq!(waited, 9);
                return;
            }
            other => panic!("{other:?}"),
        }
    }
    panic!("Intl continuation did not finish");
}

/// Synthetic fixture, not light-cljs output. Locale's ordinary properties keep
/// the existing task ownership/publication boundary despite immutable slots.
#[test]
fn intl_locale_task_ownership() {
    let rt = Runtime::new();
    let owner = rt
        .spawn(&exe("Object.freeze(new Intl.Locale('en'))"), &[])
        .unwrap();
    let RunOutcome::Complete(locale) = owner.run().unwrap() else {
        panic!("expected Locale");
    };
    assert!(matches!(
        rt.publish(&locale),
        Err(clj_quickjs::PublishError::NotShareable { .. })
    ));
    for body in [
        "value.language",
        "Intl.Locale.prototype.toString.call(value)",
        "Intl.Locale.prototype.maximize.call(value)",
        "Intl.Locale.prototype.minimize.call(value)",
        "Object.getOwnPropertyDescriptor(Intl.Locale.prototype,'region').get.call(value)",
        "new Intl.Locale(value)",
        "Intl.getCanonicalLocales(value)",
    ] {
        let code = compile_function(&format!("function read(value) {{ return {body}; }}")).unwrap();
        let caller = rt.spawn(&code, &[&locale]).unwrap();
        assert!(
            matches!(caller.run().unwrap(), RunOutcome::Failed(e) if matches!(e.kind(), VmErrorKind::ForeignObject { .. })),
            "{body}"
        );
    }
}

/// Synthetic fixtures, not light-cljs output. The division and power rows adapt
/// QuickJS tests/test_bigint.js at 7d1c13c5d8f8c6b32893786ed35cdcebd88194d2.
/// Copyright (c) 2017-2021 Fabrice Bellard
/// Copyright (c) 2017-2021 Charlie Gordon
///
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
/// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
/// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
/// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
/// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
/// THE SOFTWARE.
#[test]
fn bigint_division_remainder_and_exponentiation() {
    for src in [
        "1n / 1n === 1n && 1n % 1n === 0n && 1n ** 1n === 1n",
        "3213213213213213432453243n / 123434343439n === 26031760073331n",
        "-3213213213213213432453243n / 123434343439n === -26031760073331n",
        "-3213213213213213432453243n % -123434343439n === -26953727934n",
        "3213213213213213432453243n % 123434343439n === 26953727934n",
        "(-2n) ** 127n === -170141183460469231731687303715884105728n",
        "2n ** 127n === 170141183460469231731687303715884105728n",
        "(-256n) ** 11n === -309485009821345068724781056n",
        "7n ** 20n === 79792266297612001n",
        "7n / 2n === 3n && -7n / 2n === -3n && 7n / -2n === -3n && -7n / -2n === 3n",
        "7n % -2n === 1n && -7n % 2n === -1n && -7n % -2n === -1n",
        "0n ** 0n === 1n && 5n ** 0n === 1n && 0n ** 5n === 0n",
        "2n ** 64n === 18446744073709551616n",
        "(10n ** 600n) / (10n ** 599n) === 10n",
        "(10n ** 600n + 7n) % (10n ** 300n) === 7n",
        // Roughly 13,000-bit operands, as used by the pi example.
        "let a = 10n ** 4000n; (a + 7n) / (10n ** 2000n) === 10n ** 2000n && (a + 7n) % (10n ** 2000n) === 7n",
        "-9223372036854775808n / -1n === 9223372036854775808n && -2n % 2n === 0n",
        "3n / 5n === 0n && 3n % 5n === 3n && 0n / -5n === 0n",
        "(-1n) ** 18446744073709551617n === -1n && (-1n) ** 18446744073709551616n === 1n && 1n ** 18446744073709551616n === 1n",
        "let x = 25n; x /= 2n; x %= 5n; x **= 3n; x === 8n",
        "let o = {x: 25n}; o.x /= 2n; o['x'] %= 5n; o.x **= 3n; o.x === 8n",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
    for src in ["1n / 0n", "1n % 0n", "2n ** -1n", "0n ** -1n", "1n ** -1n"] {
        let caught = format!(
            "let caught = false; try {{ {src}; }} catch (e) {{ caught = e instanceof RangeError; }} caught"
        );
        assert_eq!(eval(&caught), ValueView::Bool(true), "{src}");
    }
    for src in [
        "1n / 1",
        "1 / 1n",
        "1n % 1",
        "1 % 1n",
        "2 ** 1n",
        "2n ** 1",
        "1n / true",
        "null % 1n",
    ] {
        let caught = format!(
            "let caught = false; try {{ {src}; }} catch (e) {{ caught = e instanceof TypeError; }} caught"
        );
        assert_eq!(eval(&caught), ValueView::Bool(true), "{src}");
    }
    assert_eq!(
        compile_script("bigint", "-2n ** 2n").unwrap_err().kind,
        SyntaxErrorKind::Invalid
    );
}

/// Synthetic fixture, not light-cljs output. Powers check the size cap before
/// allocating oversized results; failed compound stores preserve their target.
#[test]
fn bigint_power_size_limit_and_errors() {
    for src in [
        "2n ** 1048576n",
        "2n ** 18446744073709551616n",
        "3n ** 1048576n",
        "(2n ** 1048576n) * 0n",
        // The preliminary bound is exactly 1 Mbit; the checked products
        // discover that the actual result needs 1,048,577 bits.
        "let b = 3n * (2n ** 349524n); b ** 3n",
    ] {
        let caught = format!(
            "let caught = false; try {{ {src}; }} catch (e) {{ caught = e instanceof RangeError && e.message === 'BigInt is too large'; }} caught"
        );
        assert_eq!(eval(&caught), ValueView::Bool(true), "{src}");
    }
    for src in [
        "let b = 3n * (2n ** 349524n); let original = b; let caught = false; try { b **= 3n; } catch (e) { caught = e instanceof RangeError && e.message === 'BigInt is too large'; } caught && b === original",
        "let x = 2n; try { x **= 1048576n; } catch (e) {} x === 2n",
        "let x = 3n; try { x /= 0n; } catch (e) {} x === 3n",
        "let o = {x: 3n}; try { o.x %= 0n; } catch (e) {} o.x === 3n",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
    assert_eq!(
        eval("let a = 2n ** 1048575n; a / a === 1n && a % a === 0n && a / 2n === 2n ** 1048574n"),
        ValueView::Bool(true)
    );
    // The same preliminary bound can also describe a legal result.
    assert_eq!(
        eval(
            "let a = 2n ** 349525n; (a + 1n) ** 3n === 2n ** 1048575n + 3n * (2n ** 699050n) + 3n * a + 1n"
        ),
        ValueView::Bool(true)
    );
}

/// Synthetic fixtures, not light-cljs output. The upstream bitwise/shift rows
/// adapt QuickJS tests/test_bigint.js::test_bigint1/test_bigint3 at revision
/// 7d1c13c5d8f8c6b32893786ed35cdcebd88194d2; string rows compare exact values.
/// Copyright (c) 2017-2021 Fabrice Bellard
/// Copyright (c) 2017-2021 Charlie Gordon
///
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
/// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
/// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
/// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
/// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
/// THE SOFTWARE.
#[test]
fn bigint_bitwise_and_shift_operators() {
    for src in [
        "1n << 31n === 2147483648n",
        "1n << 32n === 4294967296n",
        "1n << 100n === 1267650600228229401496703205376n",
        "-1n << 100n === -1267650600228229401496703205376n",
        "1n << 100n === 0o2000000000000000000000000000000000n",
        "0x5a4653ca673768565b41f775n << 78n === 8443945299673273647701379149826607537748959488376832n",
        "-0x5a4653ca673768565b41f775n << 78n === -8443945299673273647701379149826607537748959488376832n",
        "0x5a4653ca673768565b41f775n >> 78n === 92441n",
        "-0x5a4653ca673768565b41f775n >> 78n === -92442n",
        "~0x5a653ca6n === -1516584103n",
        "(0x5a463ca6n | 0x67376856n) === 2138537206n",
        "(0x5a463ca6n & 0x67376856n) === 1107699718n",
        "(0x5a463ca6n ^ 0x67376856n) === 1030837488n",
        "-1n >> 1000n === -1n",
        "-5n >> 1n === -3n && -6n >> 1n === -3n",
        "5n << -1n === 2n && -5n << -1n === -3n && 5n >> -1n === 10n",
        "(-1n & 0xffn) === 255n",
        "(-6n | 5n) === -1n && (-6n ^ 3n) === -7n && ~-1n === 0n",
        "(1n << 64n) === 18446744073709551616n && (1n << 128n) === 340282366920938463463374607431768211456n",
        "(1n << 192n) === 6277101735386680763835789423207666416102355444464034512896n",
        "-9223372036854775808n >> 63n === -1n && -9223372036854775808n << 1n === -18446744073709551616n",
        "~9223372036854775807n === -9223372036854775808n && ~-9223372036854775809n === 9223372036854775808n",
        "let a = 1n << 100n; (-a & -1n) === -a && (-a | a) === -a && (-a ^ -1n) === a - 1n",
        "let a = 1n << 100n; (a ^ a) === 0n && (~a & a) === 0n && (a | ~a) === -1n",
        "0n << 18446744073709551616n === 0n && 0n >> -18446744073709551616n === 0n",
        "1n >> 18446744073709551616n === 0n && -5n << -18446744073709551616n === -1n",
        "let x = 3n; x <<= 70n; x >>= 69n; x |= 1n; x &= 6n; x ^= 2n; x === 4n",
        "let o = {x: 3n}; o.x <<= 70n; o['x'] >>= 69n; o.x |= 1n; o.x &= 6n; o.x ^= 2n; o.x === 4n",
        "let x = 1n; typeof ~x === 'bigint' && typeof (x << 2n) === 'bigint' && -(x <<= 2n) === -4n",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
    for src in [
        "1n >>> 0n",
        "1n >>> 0",
        "1 >>> 0n",
        "1n & 1",
        "1 & 1n",
        "1n | 1",
        "1 | 1n",
        "1n ^ 1",
        "1 ^ 1n",
        "1n << 1",
        "1 << 1n",
        "1n >> 1",
        "1 >> 1n",
        "1n & true",
        "null | 1n",
        "1n << '1'",
    ] {
        let caught = format!(
            "let caught = false; try {{ {src}; }} catch (e) {{ caught = e instanceof TypeError; }} caught"
        );
        assert_eq!(eval(&caught), ValueView::Bool(true), "{src}");
    }
}

/// Synthetic fixtures, not light-cljs output. Size checks apply to each
/// intermediate result and failed compound assignments preserve their targets.
#[test]
fn bigint_bitwise_and_shift_size_limits() {
    for src in [
        "1n << 1048576n",
        "-1n << 1048576n",
        "1n >> -1048576n",
        "1n << 18446744073709551616n",
        "1n >> -18446744073709551616n",
        "(1n << 1048576n) & 0n",
        "let max = (1n << 1048575n) + ((1n << 1048575n) - 1n); ~max",
        "let max = (1n << 1048575n) + ((1n << 1048575n) - 1n); max ^ -1n",
    ] {
        let caught = format!(
            "let caught = false; try {{ {src}; }} catch (e) {{ caught = e instanceof RangeError && e.message === 'BigInt is too large'; }} caught"
        );
        assert_eq!(eval(&caught), ValueView::Bool(true), "{src}");
    }
    for src in [
        "let a = 1n << 1048575n; a >> 1048575n === 1n && -a >> 1048575n === -1n && a >> 1048576n === 0n",
        "let max = (1n << 1048575n) + ((1n << 1048575n) - 1n); (max & -1n) === max && (max | 0n) === max && ~-max === max - 1n",
        "let x = 1n; try { x <<= 1048576n; } catch (e) {} x === 1n",
        "let o = {x: -1n}; try { o.x >>= -1048576n; } catch (e) {} o.x === -1n",
        "let x = 1n; try { x >>>= 0n; } catch (e) {} x === 1n",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
}

#[test]
fn regexp_has_indices() {
    assert_eq!(
        eval(include_str!("fixtures/js/regexp_indices.js")),
        ValueView::Bool(true)
    );
}

/// Synthetic fixture, not light-cljs output. Capture storage and indices
/// allocation must share the allowance with matching, including at success.
#[test]
fn regexp_indices_limit_is_catchable() {
    assert_eq!(
        eval(
            "let pattern = '^' + ''.padEnd(80000, '()') + 'a{120000}$'; \
             let text = ''.padEnd(120000, 'a'); \
             let ordinary = RegExp(pattern).exec(text); \
             let caught = false; try { RegExp(pattern, 'd').exec(text); } \
             catch(e) { caught = e instanceof RangeError && e.message.indexOf('step limit') >= 0; } \
             ordinary.length === 40001 && ordinary[0] === text && caught"
        ),
        ValueView::Bool(true)
    );
}

#[test]
fn regexp_unicode_sets() {
    assert_eq!(
        eval(include_str!("fixtures/js/regexp_sets.js")),
        ValueView::Bool(true)
    );
}

#[test]
fn regexp_v_outside_class_unicode_parity() {
    // Synthetic fixture, not light-cljs output.
    assert_eq!(
        eval(
            r#"
            let invalid = ['\\1', '\\c0', '(?=a)+', 'a{1,2', '\\k<x>',
                           '\\x0', '\\u{110000}', ']', '}'];
            for (let i = 0; i < invalid.length; i++) {
                let caught = false;
                try { RegExp(invalid[i], 'v'); }
                catch(e) { caught = e instanceof SyntaxError; }
                if (!caught) throw new Error('accepted Unicode syntax error ' + invalid[i]);
            }
            let m = /^(.)\1$/v.exec('😀😀');
            m[0] === '😀😀' && m[1] === '😀' &&
            /^(?<x>.)\k<x>$/v.test('😀😀') && !/^(.)\1$/v.test('😀a')
        "#
        ),
        ValueView::Bool(true)
    );
}

#[test]
fn regexp_property_escapes() {
    // Synthetic fixture, not light-cljs output. QuickJS rows retain their notice.
    assert_eq!(
        eval(include_str!("fixtures/js/regexp_properties.js")),
        ValueView::Bool(true)
    );
}

#[test]
fn regexp_case_insensitive_unicode() {
    // Synthetic fixture, not light-cljs output. The two replacement rows are
    // adapted from QuickJS; see the fixture's copyright and attribution.
    assert_eq!(
        eval(include_str!("fixtures/js/regexp_case_fold.js")),
        ValueView::Bool(true)
    );
}

#[test]
fn regexp_unicode_mode() {
    assert_eq!(
        eval(include_str!("fixtures/js/regexp_unicode.js")),
        ValueView::Bool(true)
    );
}

#[test]
fn regexp_objects_exec_and_test() {
    assert_eq!(
        eval(include_str!("fixtures/js/regexp_objects.js")),
        ValueView::Bool(true)
    );
    assert_eq!(
        eval("let a = /\0a/; a.toString() === '/\\0a/' && a.exec('\\0a')[0] === '\\0a'"),
        ValueView::Bool(true)
    );
}

/// Synthetic fixtures, not light-cljs output.
#[test]
fn regexp_flags_errors_and_internal_state() {
    for flag in ['i'] {
        for expression in [format!("/a/{flag}"), format!("RegExp('a', '{flag}')")] {
            assert_eq!(
                eval(&format!(
                    "let caught = false; try {{ {expression}; }} catch(e) {{ caught = e instanceof SyntaxError && e.message.indexOf(\"'{flag}'\") >= 0; }} caught"
                )),
                ValueView::Bool(true)
            );
        }
    }
    for src in [
        "let caught = false; try {'a'.split(/a/);} catch(e){caught = e instanceof TypeError && e.message.indexOf('RegExp separators') >= 0;} caught",
        r"let r = /(b+)c/g; let m = r.exec('abbbbbc'); let a = m[1] === 'bbbbb' && m.index === 1; let b = new RegExp('a/b').source === 'a\\/b'; r = /a/y; r.lastIndex = 1; let c = r.test('ba') && r.lastIndex === 2; let d = /a/u.test('a'); a && b && c && d",
        "let r = /a/g; RegExp(r) === r && new RegExp(r) !== r && new RegExp(r).flags === 'g' && RegExp(r, 'm').flags === 'm'",
        "let r = /a/g; let d = Object.getOwnPropertyDescriptor(r, 'lastIndex'); d.value === 0 && d.writable && !d.enumerable && !d.configurable && r instanceof RegExp && r.constructor === RegExp",
        "RegExp.prototype.source === '(?:)' && RegExp.prototype.flags === '' && RegExp.prototype.global === undefined && RegExp.prototype.toString() === '/(?:)/'",
        "let r = /./msgy; r.global && r.multiline && r.dotAll && r.sticky && !r.ignoreCase && !r.unicode && !r.unicodeSets && !r.hasIndices",
        "let m = /(?<x>a)(?<y>b)?/.exec('a'); m.groups.x === 'a' && m.groups.y === undefined && Object.getPrototypeOf(m.groups) === null && 2 in m && m[2] === undefined",
        "let m = /(?<x>a)|(?<x>b)/.exec('b'); m.groups.x === 'b'",
        "let r = /a/g; r.lastIndex = 1.9; r.exec('baa').index === 1 && r.lastIndex === 2",
        "let r = /a/g; r.lastIndex = Infinity; r.exec('a') === null && r.lastIndex === 0",
        "let r = /a/; r.lastIndex = 123; r.exec('a')[0] === 'a' && r.lastIndex === 123",
        "let r = /a/g; Object.defineProperty(r, 'lastIndex', {writable:false}); let caught = false; try {r.exec('a');} catch(e) {caught = e instanceof TypeError;} caught",
        "let r = /a/g; r.lastIndex = -10; r.test('a') && r.lastIndex === 1",
        "let r = /(?:)/g; r.lastIndex = 1; r.exec('a').index === 1 && r.lastIndex === 1",
        "let r = /./g; let s = String.fromCharCode(0xd800, 0xdc00); r.exec(s)[0].length === 1 && r.lastIndex === 1 && r.exec(s).index === 1 && r.lastIndex === 2",
        "let r = /a/; let n = 0; r.lastIndex = {valueOf(){n++; return 8;}}; r.test('a') && n === 1",
        "let r = /a/; r.exec = () => null; !r.test('a')",
        "RegExp.prototype.test.call({exec(s){return s === '3' ? {} : null;}}, 3)",
        "let caught = false; try {RegExp.prototype.test.call({exec(){return 42;}}, 'a');} catch(e){caught = e instanceof TypeError;} caught",
        "let caught = false; try {RegExp.prototype.exec.call({}, {toString(){throw 42;}});} catch(e){caught = e instanceof TypeError;} caught",
        "let r = /a/; r.exec = 0; r.test('a')",
        "RegExp.prototype.toString.call({source:'x',flags:'y'}) === '/x/y'",
        "let get = Object.getOwnPropertyDescriptor(RegExp.prototype, 'flags').get; get.call({global:1,ignoreCase:1,multiline:1,dotAll:1,sticky:1}) === 'gimsy'",
        r"new RegExp('a/b\n\r\u2028\u2029').source === 'a\\/b\\n\\r\\u2028\\u2029'",
        r"new RegExp('[/]').source === '[/]' && /a\/b/.source === 'a\\/b'",
        "let caught = false; let s = ''; for (let i = 0; i < 25; i++) s += 'a'; try {/(a+)+b/.test(s);} catch(e){caught = e instanceof RangeError && e.message.indexOf('step limit') >= 0;} caught",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
}

/// Synthetic fixture, not light-cljs output. Programs survive serialization;
/// object mutation remains task-owned and rejects cross-task publication.
#[test]
fn regexp_code_reload_and_publication() {
    let rt = Runtime::new();
    let code = Executable::load(exe("/a/g").to_spec()).unwrap();
    let owner = rt.spawn(&code, &[]).unwrap();
    let RunOutcome::Complete(value) = owner.run().unwrap() else {
        panic!("regexp")
    };
    rt.collect().unwrap();
    assert!(rt.publish(&value).is_err());
    let reader = compile_function("function read(r) {return r.exec('a');}").unwrap();
    let task = rt.spawn(&reader, &[&value]).unwrap();
    assert!(
        matches!(task.run().unwrap(), RunOutcome::Failed(e) if matches!(e.kind(), VmErrorKind::ForeignObject { .. }))
    );
}

/// Synthetic fixtures, not light-cljs output. Every observable coercion and
/// getter resumes across waits, collection and migration without replay.
#[test]
fn regexp_coercions_wait_yield_collect_and_migrate() {
    for expression in [
        "let r = /(?<n>.)/duy; r.lastIndex = {valueOf(){count++; __rt.need(188, 1); return 1;}};|(() => {let m = r.exec('🐱'); return m.indices[0][0] === 0 && m.indices[0][1] === 2 && m.indices.groups.n === m.indices[1] && m.indices[1][1] === 2;})()",
        "let m = /(?<n>a)/d.exec('ba'); let pair = m.indices.groups.n; count++; __rt.need(188, 1);|pair === m.indices[1] && pair[0] === 1 && pair[1] === 2",
        "RegExp({toString(){count++; __rt.need(188, 1); return 'a';}}, 'g').test('a')",
        "let r = /./uy; r.lastIndex = {valueOf(){count++; __rt.need(188, 1); return 1;}};|r.exec('🐱')[0] === '🐱' && r.lastIndex === 2",
        "new RegExp('a', {toString(){count++; __rt.need(188, 1); return 'g';}}).test('a')",
        "let r = /a/g; r.lastIndex = {valueOf(){count++; __rt.need(188, 1); return 1;}};|r.exec('ba').index === 1",
        "let r = /a/g;|r.exec({toString(){count++; __rt.need(188, 1); return 'a';}})[0] === 'a'",
        "RegExp.prototype.toString.call({get source(){count++; __rt.need(188, 1); return 'a';}, flags:'g'}) === '/a/g'",
        "let get = Object.getOwnPropertyDescriptor(RegExp.prototype, 'flags').get;|get.call({get global(){count++; __rt.need(188, 1); return true;}, sticky:true}) === 'gy'",
        "let r = /a/; Object.defineProperty(r, 'constructor', {get(){count++; __rt.need(188, 1); return RegExp;}});|RegExp(r) === r",
        "RegExp.prototype.test.call({get exec(){count++; __rt.need(188, 1); return () => ({});}}, 'a')",
        "RegExp.prototype.test.call({exec(){count++; __rt.need(188, 1); return null;}}, 'a') === false",
        "let caught = false; try {RegExp({toString(){count++; __rt.need(188, 1); throw 42;}});} catch(e){caught = e === 42;}|caught",
    ] {
        let rt = Runtime::new();
        // Preserve statement setup while returning its final expression.
        let (setup, result) = expression.split_once('|').unwrap_or(("", expression));
        let source = format!(
            "let count = 0; let result = (() => {{ {setup} return {result}; }})(); result && count === 1"
        );
        let mut task = rt.spawn(&exe(&source), &[]).unwrap();
        let mut waits = 0;
        let mut done = false;
        for _ in 0..1000 {
            let (next, outcome) = std::thread::spawn(move || {
                let outcome = task
                    .run_with(Budget::instructions(1), &mut NoEffects)
                    .unwrap();
                (task, outcome)
            })
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
                    let value = rt.root(Primitive::Number(0.0));
                    task = rt
                        .complete(request.key(), request.producer(), Completion::Ready(&value))
                        .unwrap()
                        .into_runnable()
                        .pop()
                        .unwrap();
                }
                RunOutcome::Complete(v) => {
                    assert_eq!(v.view(), ValueView::Bool(true), "{source}");
                    done = true;
                    break;
                }
                other => panic!("{source}: {other:?}"),
            }
        }
        assert!(done, "instruction bound: {source}");
        assert_eq!(waits, 1);
    }
}

/// Synthetic fixtures, not light-cljs output. Compilation exhaustion must
/// remain catchable, including cached failures after bytecode serialization.
#[test]
fn regexp_compilation_limit_is_catchable() {
    let pattern = "a".repeat(clj_quickjs::regexp::matcher::MAX_MATCH_STEPS);
    for constructor in ["RegExp", "new RegExp"] {
        let code = compile_function(&format!(
            "function f(pattern) {{ let caught = false; try {{ {constructor}(pattern); }} catch(e) {{ caught = e instanceof RangeError && e.message.indexOf('step limit') >= 0; }} return caught; }}"
        )).unwrap();
        let result = run(&code, &[Primitive::String(pattern.as_str().into())]);
        assert!(
            matches!(result, RunOutcome::Complete(v) if v.view() == ValueView::Bool(true)),
            "{constructor}"
        );
    }
    let source = format!(
        "let caught = false; try {{ /{pattern}/; }} catch(e) {{ caught = e instanceof RangeError && e.message.indexOf('step limit') >= 0; }} caught"
    );
    let literal = exe(&source);
    let reloaded = Executable::load(literal.to_spec()).unwrap();
    for code in [literal, reloaded] {
        assert!(
            matches!(run(&code, &[]), RunOutcome::Complete(v) if v.view() == ValueView::Bool(true))
        );
    }
}

/// Synthetic fixture, not light-cljs output. Object's tag depends on the
/// internal RegExp slot, independently of an object's prototype chain.
#[test]
fn regexp_object_tag_and_brand() {
    assert_eq!(
        eval(
            r#"
        let literal = /a/; let constructed = new RegExp('a');
        let tag = Object.prototype.toString;
        let before = tag.call(literal) === '[object RegExp]' &&
                     tag.call(constructed) === '[object RegExp]';
        Object.setPrototypeOf(literal, null);
        before && tag.call(literal) === '[object RegExp]' &&
        tag.call(RegExp.prototype) === '[object Object]' &&
        tag.call(Object.create(RegExp.prototype)) === '[object Object]'
    "#
        ),
        ValueView::Bool(true)
    );
}

/// Synthetic fixtures, not light-cljs output. RegExp split dispatch is
/// deferred and must fail explicitly for every supported creation form.
#[test]
fn string_split_rejects_regexp_separators() {
    for separator in ["/,/", "RegExp(',')", "new RegExp(',')"] {
        assert_eq!(
            eval(&format!(
                "let caught = false; try {{ 'a,b'.split({separator}); }} catch(e) {{ caught = e instanceof TypeError && e.message.indexOf('RegExp separators') >= 0; }} caught"
            )),
            ValueView::Bool(true),
            "{separator}"
        );
    }
}

/// Synthetic fixtures, not light-cljs output. Covers the StringToBigInt
/// acceptance rows also exercised by QuickJS test_bigint2 (semantic reference).
#[test]
fn bigint_function_converts_numbers_and_strings() {
    for src in [
        r#"BigInt('') === 0n && BigInt('  123') === 123n && BigInt('  123   ') === 123n"#,
        r#"BigInt(' \n\t-42 ') === -42n && BigInt('+42') === 42n"#,
        r#"BigInt('0x1f') === 31n && BigInt('0b101') === 5n && BigInt('0o17') === 15n"#,
        r#"BigInt('0XFF') === 255n && BigInt('0O17') === 15n && BigInt('0B101') === 5n"#,
        r#"BigInt(true) === 1n && BigInt(false) === 0n && BigInt(10n) === 10n"#,
        r#"BigInt(9007199254740993) === 9007199254740992n && BigInt(1e21) === 1000000000000000000000n"#,
        r#"BigInt(2**60) === 1152921504606846976n && BigInt(-0) === 0n && BigInt(-1e21) === -1000000000000000000000n"#,
        r#"typeof BigInt === 'function' && BigInt.length === 1 && BigInt.name === 'BigInt' && globalThis.BigInt === BigInt"#,
        r#"BigInt.prototype.constructor === BigInt && Object.getPrototypeOf(1n) === BigInt.prototype && (1n).constructor === BigInt"#,
        r#"Object.getPrototypeOf(BigInt.prototype) === Object.prototype && Object.getPrototypeOf(BigInt) === Function.prototype"#,
        r#"BigInt('\uFEFF\u00A0\u1680\u2000\u200A\u202F\u205F\u3000\u2028\u2029\r\v\f42\t ') === 42n"#,
        r#"BigInt('000010') === 10n && BigInt('-000') === 0n && BigInt(' \n ') === 0n"#,
        r#"let log = ''; let x = { valueOf() { log += 'v'; return {}; }, toString() { log += 's'; return '-42'; } }; BigInt(x) === -42n && log === 'vs'"#,
        r#"BigInt.call(null, '42') === 42n && BigInt.apply(null, [true]) === 1n && BigInt.bind(null, 12)() === 12n"#,
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
    for (kind, expressions) in [
        (
            "SyntaxError",
            &[
                r#"BigInt('+')"#,
                r#"BigInt('-')"#,
                r#"BigInt('\x00a')"#,
                r#"BigInt('  123  r')"#,
                r#"BigInt('-0x1')"#,
                r#"BigInt('+0b1')"#,
                r#"BigInt('1n')"#,
                r#"BigInt('1.5')"#,
                r#"BigInt('1e3')"#,
                r#"BigInt('1_0')"#,
                r#"BigInt('0x')"#,
                r#"BigInt('0b2')"#,
                r#"BigInt('0o8')"#,
                r#"BigInt('Infinity')"#,
                r#"BigInt('\u008542')"#,
                r#"BigInt('\uD800')"#,
                r#"BigInt('+ 1')"#,
            ][..],
        ),
        (
            "RangeError",
            &[
                "BigInt(1.5)",
                "BigInt(NaN)",
                "BigInt(Infinity)",
                "BigInt(-Infinity)",
                "BigInt(5e-324)",
            ][..],
        ),
        (
            "TypeError",
            &[
                "BigInt(undefined)",
                "BigInt(null)",
                "BigInt()",
                "BigInt(Symbol())",
                "new BigInt(1)",
                "new (BigInt.bind(null))(1)",
                "BigInt({valueOf() { return {}; }, toString() { return {}; }})",
            ][..],
        ),
    ] {
        for expression in expressions {
            assert_eq!(
                eval(&format!(
                    "let caught = false; try {{ {expression}; }} catch(e) {{ caught = e instanceof {kind}; }} caught"
                )),
                ValueView::Bool(true),
                "{expression}"
            );
        }
    }
}

/// Synthetic fixtures, not light-cljs output. StringToBigInt comparisons
/// preserve exact integers; invalid strings are unordered and unequal.
#[test]
fn bigint_string_comparisons_use_integer_grammar() {
    for src in [
        "1n < '2'",
        "'1' < 2n",
        "1n == '1'",
        "'1' == 1n",
        "1n <= {valueOf() { return '2'; }}",
        "({valueOf() { return '1'; }}) == 1n",
        "9007199254740993n == '9007199254740993' && 9007199254740993n > '9007199254740992'",
        "'9007199254740993' > 9007199254740992n && '9007199254740993' <= 9007199254740993n",
        "0n == '' && 0n == '  ' && -42n == '\\n-42\\t' && 16n == '0x10'",
        "'0b11' >= 3n && '0o10' < 9n && 2n !== '2'",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
    for text in ["1.5", "1e3", "-0x1", "1n", "+", "r", "1_0", "0x"] {
        assert_eq!(
            eval(&format!(
                "let s = '{text}'; !(1n == s) && !(s == 1n) && 1n != s && !(1n < s) && !(s < 1n) && !(1n <= s) && !(s <= 1n) && !(1n > s) && !(s > 1n) && !(1n >= s) && !(s >= 1n)"
            )),
            ValueView::Bool(true),
            "{text}"
        );
    }
}

/// Synthetic fixtures, not light-cljs output. The magnitude policy applies
/// to parsed strings too; invalid strings retain SyntaxError precedence.
#[test]
fn bigint_string_parsing_enforces_size_cap() {
    use clj_quickjs::MAX_BIGINT_BITS;
    let code =
        compile_function("function f(s) { return BigInt(s) === (1n << 1048575n); }").unwrap();
    for text in [
        format!("0x8{}", "0".repeat(MAX_BIGINT_BITS / 4 - 1)),
        format!("0b1{}", "0".repeat(MAX_BIGINT_BITS - 1)),
    ] {
        assert!(
            matches!(run(&code, &[Primitive::String(text.as_str().into())]), RunOutcome::Complete(v) if v.view() == ValueView::Bool(true))
        );
    }
    let caught = compile_function("function f(s) { try { BigInt(s); } catch(e) { return e instanceof RangeError && e.message === 'BigInt is too large'; } return false; }").unwrap();
    let oversized = [
        format!("0x1{}", "0".repeat(MAX_BIGINT_BITS / 4)),
        "9".repeat(MAX_BIGINT_BITS * 30103 / 100000 + 2),
    ];
    for text in &oversized {
        assert!(
            matches!(run(&caught, &[Primitive::String(text.as_str().into())]), RunOutcome::Complete(v) if v.view() == ValueView::Bool(true))
        );
    }
    let malformed = compile_function("function f(s) { try { BigInt(s); } catch(e) { return e instanceof SyntaxError && !(1n == s) && !(1n <= s); } return false; }").unwrap();
    let text = format!("{}r", oversized[1]);
    assert!(
        matches!(run(&malformed, &[Primitive::String(text.as_str().into())]), RunOutcome::Complete(v) if v.view() == ValueView::Bool(true))
    );
    assert_eq!(
        eval("BigInt(Number.MAX_VALUE) === (1n << 1024n) - (1n << 971n)"),
        ValueView::Bool(true)
    );
}

/// Synthetic fixtures, not light-cljs output. Decimal parsing must yield
/// during limb updates as well as during scans; ToPrimitive runs only once.
#[test]
fn bigint_string_conversion_waits_yields_collects_and_migrates() {
    for expression in [
        "BigInt(value) === 10n ** 3999n",
        "value == 10n ** 3999n",
        "10n ** 3999n <= value",
        "value >= 10n ** 3999n",
    ] {
        let rt = Runtime::new();
        let source = format!(
            "let calls = 0; let value = {{valueOf() {{ calls++; return __rt.need(2011, 1); }} }}; let ok = {expression}; ok && calls === 1"
        );
        let mut task = rt.spawn(&exe(&source), &[]).unwrap();
        let text = format!("1{}", "0".repeat(3999));
        let mut waited = 0;
        let mut yields = 0;
        let mut complete = false;
        for _ in 0..1000 {
            let (next, outcome) = std::thread::spawn(move || {
                let outcome = task
                    .run_with(Budget::instructions(1), &mut NoEffects)
                    .unwrap();
                (task, outcome)
            })
            .join()
            .unwrap();
            task = next;
            match outcome {
                RunOutcome::Yielded => {
                    yields += 1;
                    rt.collect().unwrap();
                }
                RunOutcome::Waiting(_) => {
                    waited += 1;
                    assert_eq!(waited, 1);
                    let request = task.take_producer_requests().pop().unwrap();
                    let value = rt.root(Primitive::String(text.as_str().into()));
                    task = rt
                        .complete(request.key(), request.producer(), Completion::Ready(&value))
                        .unwrap()
                        .into_runnable()
                        .pop()
                        .unwrap();
                }
                RunOutcome::Complete(value) => {
                    assert_eq!(value.view(), ValueView::Bool(true), "{expression}");
                    assert_eq!(waited, 1);
                    assert!(yields > 100, "decimal limb updates must yield: {yields}");
                    complete = true;
                    break;
                }
                other => panic!("{expression}: {other:?}"),
            }
        }
        assert!(complete, "{expression}");
    }
}

/// Synthetic fixtures, not light-cljs output. Both conversions may suspend;
/// long strings use budgeted parsing even when the requested width is zero.
#[test]
fn bigint_as_n_waits_yields_collects_and_migrates() {
    for expression in [
        "BigInt.asIntN(bits, value) === -1n",
        "BigInt.asUintN(bits, value) === (1n << 65n) - 1n",
        "BigInt.asIntN(0 * bits, value) === 0n",
    ] {
        let rt = Runtime::new();
        let source = format!(
            "let log = ''; let bits = {{valueOf() {{log += 'b'; return __rt.need(2041, 1);}}}}; let value = {{valueOf() {{log += 'n'; return __rt.need(2042, 1);}}}}; let ok = {expression}; ok && log === 'bn'"
        );
        let mut task = rt.spawn(&exe(&source), &[]).unwrap();
        let text = format!("0x{}", "f".repeat(8000));
        let mut waited = 0;
        let mut yields = 0;
        let mut complete = false;
        for _ in 0..1000 {
            let (next, outcome) = std::thread::spawn(move || {
                let outcome = task
                    .run_with(Budget::instructions(1), &mut NoEffects)
                    .unwrap();
                (task, outcome)
            })
            .join()
            .unwrap();
            task = next;
            match outcome {
                RunOutcome::Yielded => {
                    yields += 1;
                    rt.collect().unwrap();
                }
                RunOutcome::Waiting(_) => {
                    waited += 1;
                    assert!(waited <= 2);
                    rt.collect().unwrap();
                    let request = task.take_producer_requests().pop().unwrap();
                    let value = rt.root(if waited == 1 {
                        Primitive::Number(65.0)
                    } else {
                        Primitive::String(text.as_str().into())
                    });
                    task = rt
                        .complete(request.key(), request.producer(), Completion::Ready(&value))
                        .unwrap()
                        .into_runnable()
                        .pop()
                        .unwrap();
                }
                RunOutcome::Complete(value) => {
                    assert_eq!(value.view(), ValueView::Bool(true), "{expression}");
                    assert_eq!(waited, 2);
                    assert!(yields > 100, "string parsing must yield: {yields}");
                    complete = true;
                    break;
                }
                other => panic!("{expression}: {other:?}"),
            }
        }
        assert!(complete, "{expression}");
    }
}

/// Synthetic fixtures, not light-cljs output. Every primitive result takes
/// the same constructor completion path after object coercion, exactly once.
#[test]
fn bigint_object_coercion_completes_with_each_primitive_kind() {
    for (primitive, expected) in [
        ("42", "42n"),
        ("-0", "0n"),
        ("1e21", "1000000000000000000000n"),
        ("true", "1n"),
        ("false", "0n"),
        ("18446744073709551616n", "18446744073709551616n"),
    ] {
        let source = format!(
            "let calls = 0; let x = {{valueOf() {{ calls++; return {primitive}; }}, toString() {{ throw 99; }} }}; BigInt(x) === {expected} && calls === 1"
        );
        assert_eq!(eval(&source), ValueView::Bool(true), "{primitive}");
    }
    for (primitive, kind) in [
        ("1.5", "RangeError"),
        ("NaN", "RangeError"),
        ("Infinity", "RangeError"),
        ("undefined", "TypeError"),
        ("null", "TypeError"),
        ("Symbol()", "TypeError"),
    ] {
        let source = format!(
            "let calls = 0; let x = {{valueOf() {{ calls++; return {primitive}; }}, toString() {{ throw 99; }} }}; let caught = false; try {{ BigInt(x); }} catch (e) {{ caught = e instanceof {kind}; }} caught && calls === 1"
        );
        assert_eq!(eval(&source), ValueView::Bool(true), "{primitive}");
    }
    assert_eq!(
        eval(
            "let token = {}; let calls = 0; let caught = false; try { BigInt({valueOf() { calls++; throw token; }}); } catch(e) { caught = e === token; } caught && calls === 1"
        ),
        ValueView::Bool(true)
    );
    assert_eq!(
        eval("('toString' in BigInt.prototype) && ('valueOf' in BigInt.prototype)"),
        ValueView::Bool(true)
    );
}
/// Synthetic fixtures, not light-cljs output. The setUTCHours timestamp row
/// adapts QuickJS tests/test_builtin.js::test_date, revision 7d1c13c5d8f8.
/// Copyright (c) 2017-2021 Fabrice Bellard and Charlie Gordon.
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
fn date_utc_fields() {
    for src in [
        "let d = new Date(1506098258091); d.setUTCHours(18, 10, 11) === 1506103811091 && d.toISOString() === '2017-09-22T18:10:11.091Z'",
        "let d = new Date(1506098258091); d.setUTCHours(25); d.toISOString() === '2017-09-23T01:37:38.091Z'",
        "let d = new Date(Date.UTC(2017, 0, 15)); d.setUTCMonth(1, 30); d.toISOString() === '2017-03-02T00:00:00.000Z'",
        "let d = new Date(NaN); d.setUTCFullYear(2000) === 946684800000 && d.toISOString() === '2000-01-01T00:00:00.000Z'",
        "let d = new Date(NaN); Number.isNaN(d.setUTCMinutes(1)) && Number.isNaN(d.getTime())",
        "let d = new Date(1506098258091); d.setUTCDate(0); d.toISOString() === '2017-08-31T16:37:38.091Z'",
        "let d = new Date(1506098258091); d.setUTCMinutes(-1, 60, 1000); d.toISOString() === '2017-09-22T16:00:01.000Z'",
        "let d = new Date(1506098258091); d.setUTCSeconds(60); d.toISOString() === '2017-09-22T16:38:00.091Z'",
        "let d = new Date(1506098258091); d.setUTCMilliseconds(-1.9); d.toISOString() === '2017-09-22T16:37:37.999Z'",
        "let d = new Date(0); d.setUTCFullYear(1); d.toISOString() === '0001-01-01T00:00:00.000Z'",
        "let d = new Date(-1); d.setUTCFullYear(2000, 1, 29); d.toISOString() === '2000-02-29T23:59:59.999Z'",
        "let d = new Date(8640000000000000); d.setUTCMilliseconds(1); Number.isNaN(d.getTime())",
        "let d = new Date(-8640000000000000); d.setUTCMilliseconds(-1); Number.isNaN(d.getTime())",
        "let d = new Date(0); Object.is(d.setUTCMilliseconds(-0.9), 0)",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
    for (name, expected) in [
        ("getUTCFullYear", 2017),
        ("getUTCMonth", 8),
        ("getUTCDate", 22),
        ("getUTCDay", 5),
        ("getUTCHours", 16),
        ("getUTCMinutes", 37),
        ("getUTCSeconds", 38),
        ("getUTCMilliseconds", 91),
    ] {
        let src = format!(
            "new Date(1506098258091).{name}() === {expected} && Number.isNaN(new Date(NaN).{name}())"
        );
        assert_eq!(eval(&src), ValueView::Bool(true), "{src}");
    }
}

/// Synthetic fixtures, not light-cljs output: receiver validation, descriptors,
/// observable ToNumber order and snapshotting before callbacks mutate the slot.
#[test]
fn date_utc_fields_coercion() {
    for (name, args) in [
        ("setUTCFullYear", &["2017", "8", "22"][..]),
        ("setUTCMonth", &["8", "22"][..]),
        ("setUTCHours", &["16", "37", "38", "91"][..]),
        ("setUTCMinutes", &["37", "38", "91"][..]),
        ("setUTCSeconds", &["38", "91"][..]),
    ] {
        for optional in 1..args.len() {
            let omitted = args[..optional].join(", ");
            let src = format!(
                "let d = new Date(1506098258091); d.{name}({omitted}) === 1506098258091 && d.getTime() === 1506098258091"
            );
            assert_eq!(eval(&src), ValueView::Bool(true), "{src}");

            let mut explicit: Vec<String> = args.iter().map(|s| (*s).into()).collect();
            explicit[optional] = "undefined".into();
            for later in explicit.iter_mut().skip(optional + 1) {
                *later = format!("{{valueOf() {{calls++; return {later};}}}}");
            }
            let later_count = args.len() - optional - 1;
            let explicit = explicit.join(", ");
            let src = format!(
                "let calls = 0; let d = new Date(1506098258091); Number.isNaN(d.{name}({explicit})) && Number.isNaN(d.getTime()) && calls === {later_count}"
            );
            assert_eq!(eval(&src), ValueView::Bool(true), "{src}");
        }
    }
    for (name, length) in [
        ("getUTCFullYear", 0),
        ("getUTCMonth", 0),
        ("getUTCDate", 0),
        ("getUTCDay", 0),
        ("getUTCHours", 0),
        ("getUTCMinutes", 0),
        ("getUTCSeconds", 0),
        ("getUTCMilliseconds", 0),
        ("setUTCFullYear", 3),
        ("setUTCMonth", 2),
        ("setUTCDate", 1),
        ("setUTCHours", 4),
        ("setUTCMinutes", 3),
        ("setUTCSeconds", 2),
        ("setUTCMilliseconds", 1),
    ] {
        let src = format!(
            "let m = Date.prototype.{name}; let p = Object.getOwnPropertyDescriptor(Date.prototype, '{name}'); m.name === '{name}' && m.length === {length} && p.value === m && p.writable && !p.enumerable && p.configurable"
        );
        assert_eq!(eval(&src), ValueView::Bool(true), "{src}");
        for receiver in [
            "{}",
            "Date.prototype",
            "Object.create(new Date(0))",
            "null",
            "1",
        ] {
            let src = format!(
                "let converted = false; let caught = false; try {{Date.prototype.{name}.call({receiver}, {{valueOf() {{converted = true; return 1;}}}});}} catch(e) {{caught = e instanceof TypeError;}} caught && !converted"
            );
            assert_eq!(eval(&src), ValueView::Bool(true), "{src}");
        }
        let src = format!(
            "let caught = false; try {{new Date.prototype.{name}();}} catch(e) {{caught = e instanceof TypeError;}} caught"
        );
        assert_eq!(eval(&src), ValueView::Bool(true), "{src}");
        if length > 0 {
            for args in ["", "undefined", "NaN", "Infinity"] {
                let src = format!(
                    "let d = new Date(0); Number.isNaN(d.{name}({args})) && Number.isNaN(d.getTime())"
                );
                assert_eq!(eval(&src), ValueView::Bool(true), "{src}");
            }
            let src = format!(
                "let d = new Date(1); let caught = false; try {{d.{name}(Symbol());}} catch(e) {{caught = e instanceof TypeError;}} caught && d.getTime() === 1"
            );
            assert_eq!(eval(&src), ValueView::Bool(true), "{src}");
        }
    }
    for src in [
        "let log = ''; let d = new Date(NaN); let result = d.setUTCHours({valueOf() {log += 'h'; return NaN;}}, {valueOf() {log += 'm'; return 1;}}, {valueOf() {log += 's'; return 2;}}, {valueOf() {log += 'f'; return 3;}}, {valueOf() {throw 4;}}); log === 'hmsf' && Number.isNaN(result)",
        "let log = ''; let d = new Date(1); try {d.setUTCMinutes({valueOf() {log += 'm'; return 1;}}, {valueOf() {log += 's'; throw 2;}}, {valueOf() {log += 'f'; return 3;}});} catch(e) {log += e;} log === 'ms2' && d.getTime() === 1",
        "let log = ''; try {new Date(NaN).setUTCMinutes(Symbol(), {valueOf() {log += 's'; return 1;}});} catch(e) {log += e instanceof TypeError;} log === 'true'",
        "let d = new Date(1506098258091); d.setUTCHours({valueOf() {d.setTime(0); return 18;}}); d.toISOString() === '2017-09-22T18:37:38.091Z'",
        "let d = new Date(NaN); let r = d.setUTCMinutes({valueOf() {d.setTime(0); return 1;}}); Number.isNaN(r) && d.getTime() === 0",
        "let d = new Date(NaN); d.setUTCFullYear({valueOf() {d.setTime(100000); return 2000;}}); d.toISOString() === '2000-01-01T00:00:00.000Z'",
        "let d = new Date(1); Object.freeze(d); d.setUTCSeconds(1) === 1001 && d.getUTCSeconds() === 1",
        "let d = new Date(1); Object.setPrototypeOf(d, null); Date.prototype.setUTCSeconds.call(d, 1) === 1001 && Date.prototype.getUTCSeconds.call(d) === 1",
        "new Date(-1).getUTCDay({valueOf() {throw 1;}}) === 3",
        "let d = new Date(1); let caught = false; try {d.setUTCSeconds('5'.padStart(4097, ' '));} catch(e) {caught = e instanceof RangeError;} caught && d.getTime() === 1",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
}

/// Synthetic fixture, not light-cljs output. Pending setter receivers and later
/// arguments survive tiny budgets, dependency waits, collection and migration.
#[test]
fn date_utc_fields_waits_collects_and_migrates() {
    let rt = Runtime::new();
    let src = r"
        let log = '';
        let result = new Date(1506098258091).setUTCHours(
            {get valueOf() {log += 'g'; __rt.need(1, 1);
                return function() {log += 'h'; __rt.need(2, 1); return 18;};}},
            {valueOf() {log += 'm'; __rt.need(3, 1); return 10;}},
            {valueOf() {log += 's'; __rt.need(4, 1); return 11;}});
        let invalid = new Date(NaN);
        let nan = invalid.setUTCMinutes({valueOf() {log += 'i'; __rt.need(5, 1); return 1;}});
        invalid.setUTCFullYear({valueOf() {log += 'y'; __rt.need(6, 1); return 2000;}});
        result === 1506103811091 && Number.isNaN(nan) && log === 'ghmsiy'
            && invalid.toISOString() === '2000-01-01T00:00:00.000Z'
    ";
    let mut task = rt.spawn(&exe(src), &[]).unwrap();
    let mut waited = 0;
    let mut complete = false;
    for _ in 0..2000 {
        let (next, outcome) = std::thread::spawn(move || {
            let outcome = task
                .run_with(Budget::instructions(1), &mut NoEffects)
                .unwrap();
            (task, outcome)
        })
        .join()
        .unwrap();
        task = next;
        match outcome {
            RunOutcome::Yielded => {
                rt.collect().unwrap();
            }
            RunOutcome::Waiting(_) => {
                waited += 1;
                assert!(waited <= 6);
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
            RunOutcome::Complete(value) => {
                assert_eq!(value.view(), ValueView::Bool(true));
                complete = true;
                break;
            }
            other => panic!("{other:?}"),
        }
    }
    assert!(complete);
    assert_eq!(waited, 6);
}

// Adapted assertions from QuickJS tests/test_builtin.js::test_typed_array,
// revision 7d1c13c5d8f8c6b32893786ed35cdcebd88194d2.
// Synthetic fixture, not light-cljs output.
// QuickJS Javascript Engine
// Copyright (c) 2017-2021 Fabrice Bellard
// Copyright (c) 2017-2021 Charlie Gordon
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL
// THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
// THE SOFTWARE.
#[test]
fn typed_array_methods() {
    assert_eq!(
        eval(
            r#"
        let a = new Uint8Array(4);
        for (let i = 0; i < a.length; i++) a[i] = i;
        let ok = a.join(",") === "0,1,2,3";
        a = new Uint8ClampedArray(4);
        a[0] = -100; a[1] = 1.5; a[2] = 0.5; a[3] = 1233.5;
        ok = ok && a.toString() === "0,2,0,255";
        let buffer = new ArrayBuffer(16);
        new Uint32Array(buffer, 12, 1)[0] = -1;
        new Uint16Array(buffer, 2)[0] = -1;
        new Float16Array(buffer, 8, 1)[0] = 1;
        new Float32Array(buffer, 8, 1)[0] = 1;
        ok = ok && new Uint8Array(buffer).toString() === "0,0,255,255,0,0,0,0,0,0,128,63,255,255,255,255";
        a = new Uint8Array([1, 2, 3, 4]);
        ok = ok && a.toString() === "1,2,3,4";
        a.set([10, 11], 2);
        ok = ok && a.toString() === "1,2,10,11";
        let caught = false;
        try { a.set([1, 2, 3], 3); } catch (e) { caught = e instanceof RangeError; }
        ok = ok && caught && a.toString() === "1,2,10,11";
        buffer = new ArrayBuffer(16);
        a = new Uint8Array(buffer);
        a.fill(42);
        ok && a[0] === 42 && Uint8Array.prototype.toString === Array.prototype.toString &&
        new Uint8Array(new Int8Array([-1])).toString() === "255"
    "#
        ),
        ValueView::Bool(true)
    );
}

#[test]
fn typed_array_from_iterable() {
    // Synthetic fixture, not light-cljs output.
    assert_eq!(
        eval(
            r#"
        let source = {};
        source[Symbol.iterator] = function () {
            let i = 0;
            return {next() { i++; return {done: i > 3, value: i}; }};
        };
        new Uint8Array(source).toString() === '1,2,3' &&
        new Uint8Array({length: 2, 0: 7, 1: 8}).toString() === '7,8'
    "#
        ),
        ValueView::Bool(true)
    );
}

#[test]
fn typed_array_methods_budget() {
    // Synthetic fixture, not light-cljs output.
    struct Started(bool);
    impl Host for Started {
        fn effect(&mut self, call: HostCall) -> Result<Primitive, String> {
            self.0 = true;
            Ok(call.arg)
        }
    }
    for source in [
        "let a = new Uint8Array(100000); __rt.effect(168,0); a.join(',').length === 199999",
        "let a = new Uint8Array(100000); __rt.effect(168,0); a.fill(42); a[99999] === 42",
        "let a = new Uint8Array(100000); let b = new Uint8Array(100000); b.fill(17); __rt.effect(168,0); a.set(b); a[99999] === 17",
        "let a = new Array(100000); __rt.effect(168,0); new Uint8Array(a)[99999] === 0",
    ] {
        let rt = Runtime::new();
        let task = rt.spawn(&exe(source), &[]).unwrap();
        let mut yields = 0;
        let mut complete = false;
        let mut host = Started(false);
        for _ in 0..20000 {
            match task.run_with(Budget::instructions(100), &mut host).unwrap() {
                RunOutcome::Yielded => {
                    yields += usize::from(host.0);
                    if host.0 && yields == 1 {
                        rt.collect().unwrap();
                    }
                }
                RunOutcome::Complete(value) => {
                    assert_eq!(value.view(), ValueView::Bool(true), "{source}");
                    complete = true;
                    break;
                }
                other => panic!("{source}: {other:?}"),
            }
        }
        assert!(complete && yields > 0, "{source}: {yields} yields");
    }
}

#[test]
fn typed_array_method_edges() {
    // Synthetic fixtures, not light-cljs output.
    for source in [
        "let a = new Uint8Array([1,2,3,4]); a.set(new Uint8Array(a.buffer, 0, 3), 1); a.join() === '1,1,2,3'",
        "let a = new Uint8Array([1,2,3,4]); a.set(new Uint8Array(a.buffer, 1, 3)); a.join() === '2,3,4,4'",
        "let b = new ArrayBuffer(8); let a = new Uint8Array(b); a.set([1,2,3,4]); new Uint16Array(b).set(new Uint8Array(b,0,4)); a.join() === '1,0,2,0,3,0,4,0'",
        "let a = new Uint8Array([1,2,3]); let n = 0; let r = a.fill({valueOf() { n++; return 257; }}, -2, Infinity); r === a && n === 1 && a.join() === '1,1,1'",
        "let a = new Uint8Array([1,2,3]); a.fill(9,-Infinity,-1); a.join() === '9,9,3'",
        "let a = new Uint8Array([1,2,3]); a.fill(9,2,1); a.set([8],1.9); a.join() === '1,8,3'",
        "let a = new Uint8Array(3); a.set('257'); a.join() === '2,5,7'",
        "let a = new Uint8Array([7,8]); Object.defineProperty(a,'length',{value:0}); a.join() === '7,8'",
        "let a = new Uint8Array([7,8]); a.join({toString() { a.buffer.transfer(); return '-'; }}) === '-'",
        "let a = new Uint8Array([7,8]); a.join = function () { return 'custom'; }; a.toString() === 'custom'",
        "let a = new Uint8Array([7,8]); a[Symbol.iterator] = function () { throw 42; }; new Uint8Array(a).join() === '7,8'",
        "let log = ''; let s = {length:2,get 0() {log+='a'; return {valueOf() {log+='b'; return 7; }};},get 1() {log+='c'; return 8;}}; new Uint8Array(s).join() === '7,8' && log === 'abc'",
        "let log = ''; let s = {}; s[Symbol.iterator] = function () { let i = 0; return { next() {log += 'n'; i++; return {done:i>2,value:{valueOf() {log+='v'; return i;}}};}}; }; new Uint8Array(s).join() === '3,3' && log === 'nnnvv'",
        "let a = new Uint8Array(1); let n = 0; let caught = false; try { a.set({get length() { n++; return 0; }}, Infinity); } catch(e) {caught = e instanceof RangeError;} caught && n === 1",
        "let a = new Uint8Array([1]); let caught = false; try { a.set({length:2,get 0() {throw 42;}}); } catch(e) {caught = e instanceof RangeError;} caught && a[0] === 1",
        "let a = new Uint8Array(2); let b = a.buffer; a.set({length:2,get 0() {b.transfer(); return 7;},get 1() {return 8;}}); a[0] === undefined",
        "let a = new Uint8Array([1,2]); let n = 0; try { a.set({length:2,0:7,get 1() {throw 42;}}); } catch(e) {n=e;} a.join() === '7,2' && n === 42",
        "Uint8Array.prototype.set.length === 1 && Uint8Array.prototype.fill.length === 1 && Uint8Array.prototype.join.length === 1 && Uint8Array.prototype.join !== Array.prototype.join",
    ] {
        assert_eq!(eval(source), ValueView::Bool(true), "{source}");
    }
    for (source, kind) in [
        ("Uint8Array.prototype.set.call([],[])", "TypeError"),
        ("Uint8Array.prototype.fill.call([],1)", "TypeError"),
        ("Uint8Array.prototype.join.call([])", "TypeError"),
        (
            "let a=new Uint8Array(1); a.buffer.transfer(); a.set([])",
            "TypeError",
        ),
        (
            "let a=new Uint8Array(1); a.buffer.transfer(); a.fill(1)",
            "TypeError",
        ),
        (
            "let a=new Uint8Array(1); a.buffer.transfer(); a.join()",
            "TypeError",
        ),
        (
            "let a=new Uint8Array(1); a.fill({valueOf() {a.buffer.transfer(); return 1;}})",
            "TypeError",
        ),
        (
            "let a=new Uint8Array(1); a.set([], {valueOf() {a.buffer.transfer(); return 0;}})",
            "TypeError",
        ),
        (
            "let a=new Uint8Array(1); a.buffer.transfer(); new Uint8Array(a)",
            "TypeError",
        ),
        (
            "let s={}; s[Symbol.iterator]=1; new Uint8Array(s)",
            "TypeError",
        ),
        (
            "let s={}; s[Symbol.iterator]=function () {return 1;}; new Uint8Array(s)",
            "TypeError",
        ),
        (
            "let s={}; s[Symbol.iterator]=function () {return {next() {return 1;}};}; new Uint8Array(s)",
            "TypeError",
        ),
        ("new Uint8Array([1n])", "TypeError"),
        ("new Uint8Array(1).set([1n])", "TypeError"),
        ("new Uint8Array(1).fill(1n)", "TypeError"),
        ("new Uint8Array(1).set(null)", "TypeError"),
        ("new Uint8Array(1).set([], -1)", "RangeError"),
    ] {
        let source = format!(
            "let caught=false; try {{{source};}} catch(e) {{caught=e instanceof {kind};}} caught"
        );
        assert_eq!(eval(&source), ValueView::Bool(true), "{source}");
    }
}

#[test]
fn typed_array_copy_preserves_nan_payloads() {
    // Synthetic fixture, not light-cljs output. Number conversion canonicalizes
    // NaNs, while same-element-type constructor/set must copy their raw bits.
    for (kind, words) in [
        ("Float16Array", "[0x7c01,0xfe02]"),
        ("Float32Array", "[0x0001,0x7f80,0x1234,0xffc0]"),
        ("Float64Array", "[0x0001,0,0,0x7ff0,0x1234,0,0,0xfff8]"),
    ] {
        let source = format!(
            r#"
            let words = new Uint16Array({words});
            let a = new {kind}(words.buffer);
            let copy = new {kind}(a);
            let setCopy = new {kind}(a.length);
            setCopy.set(a);
            new Uint16Array(copy.buffer).join() === words.join() &&
            new Uint16Array(setCopy.buffer).join() === words.join()
        "#
        );
        assert_eq!(eval(&source), ValueView::Bool(true), "{source}");
    }
}

#[test]
fn typed_array_array_source_collection_order() {
    // Synthetic fixtures, not light-cljs output. Default array iteration
    // gathers values before numeric conversion and rereads length each time.
    for source in [
        "let a = [{valueOf() {a[1] = 9; return 1;}},2]; new Uint8Array(a).join() === '1,2'",
        "let a = [1,2]; Object.defineProperty(a,'0',{get() {a.length=1; return 7;}}); new Uint8Array(a).join() === '7'",
        "let a = [1]; Object.defineProperty(a,'0',{get() {a[1]=8; return 7;}}); new Uint8Array(a).join() === '7,8'",
        "let a = [{valueOf() {a[1] = 9; return 1;}},2]; a[Symbol.iterator] = undefined; new Uint8Array(a).join() === '1,9'",
        "let a = [{valueOf() {a[1] = 9; return 1;}},2]; a[Symbol.iterator] = null; new Uint8Array(a).join() === '1,9'",
        "let a = [{valueOf() {a[1] = 9; return 1;}},2]; Object.defineProperty(a,Symbol.iterator,{get() {return undefined;}}); new Uint8Array(a).join() === '1,9'",
        "let a = [{valueOf() {a[1] = 9; return 1;}},2]; Object.setPrototypeOf(a,null); new Uint8Array(a).join() === '1,9'",
        "let a = [{valueOf() {a[1] = 9; return 1;}},2]; Object.setPrototypeOf(a,Object.create(Array.prototype)); new Uint8Array(a).join() === '1,2'",
        "let a = Object.create(Array.prototype); a.length=2; a[0]={valueOf() {a[1]=9; return 1;}}; a[1]=2; new Uint8Array(a).join() === '1,2'",
        "let a = [1,2]; a[Symbol.iterator]=function () {let n=0; return {next() {n++; return {done:n>1,value:42};}};}; new Uint8Array(a).join() === '42'",
        "let a = [{valueOf() {a[1] = 9; return 1;}},2]; let b = new Uint8Array(2); b.set(a); b.join() === '1,9'",
    ] {
        assert_eq!(eval(source), ValueView::Bool(true), "{source}");
    }
}
/// Synthetic fixtures, not light-cljs output. Replacement positions and
/// captures are UTF-16 code units; dollar substitutions follow GetSubstitution.
#[test]
fn string_replace() {
    for source in [
        r#""abcabc".replace("b", "X") === "aXcabc""#,
        r#""abcabc".replaceAll("b", "X") === "aXcaXc""#,
        r#""ab".replaceAll("", "-") === "-a-b-""#,
        r#""aAbB".replace(/a/g, "X") === "XAbB""#,
        r#""abc".replace(/(?:)/g, "-") === "-a-b-c-""#,
        r#""abc".replace(/b/, "[$&$`$'$$]") === "a[bac$]c""#,
        r#""abc".replace(/(b)/, "$1$2$01") === "ab$2bc""#,
        r#""abc".replace(/(?<x>b)/, "<$<x>>") === "a<b>c""#,
        r#"let caught = false; try { "aa".replaceAll(/a/, "b"); } catch(e) { caught = e instanceof TypeError; } caught"#,
        r#"let r = /a/g; r.lastIndex = 99; "aa".replace(r, "b") === "bb" && r.lastIndex === 0"#,
        r#"let r = /(?:)/g; "ab".replaceAll(r, "-") === "-a-b-" && r.lastIndex === 0"#,
        r#""aaa".replaceAll("aa", "X") === "Xa" && "".replaceAll("", "-") === "-""#,
        r#""abc".replace("", "-") === "-abc" && "abc".replace("z", "x") === "abc""#,
        r#""b".replace(/(a)?b/, "<$1>") === "<>" && "b".replace(/b/, "$<x>$0$00$01$10$") === "$<x>$0$00$01$10$""#,
        r#""ab".replace(/(a)(b)/, "$12-$02-$99") === "a2-b-$99""#,
        r#""ab".replace(/(?<x>a)(?<y>z)?/, "$<x>:$<y>:$<absent>:$<x") === "a:::$<xb""#,
        r#""ab".replace("a", "$1$<x>$&") === "$1$<x>ab""#,
        r#"let r = /b/y; r.lastIndex = 1; "abc".replace(r, "X") === "aXc" && r.lastIndex === 2"#,
        r#"let r = /b/; r.lastIndex = 2; "abc".replace(r, "X") === "aXc" && r.lastIndex === 2"#,
        r#""\uD800\uDC00".replaceAll("", "-") === "-\uD800-\uDC00-""#,
        r#"let r = /a/g; let n = 0; "aa".replace(r, function(m, p, s) { n++; return m + p + s.length; }) === "a02a12" && n === 2"#,
        r#""ab".replaceAll("", (m, p, s) => p) === "0a1b2""#,
        r#""ab".replace(/(?<x>a)(z)?/, function(m, a, z, p, s, groups) { return m === a && z === undefined && p === 0 && s === 'ab' && groups.x === a && Object.getPrototypeOf(groups) === null ? '$&' : 'bad'; }) === "$&b""#,
        r#"let r = /a/g; let n = 0; "aa".replace(r, () => { n++; let zero = r.lastIndex === 0; r.lastIndex = 77; return zero ? 'X' : 'Y'; }) === "XY" && n === 2 && r.lastIndex === 77"#,
        r#"let caught = false; try { 'x'.replace('x', () => { throw 42; }); } catch(e) { caught = e === 42; } caught"#,
        r#"let log = ''; let r = String.prototype.replace.call({toString(){log += 't'; return 'aba';}}, {toString(){log += 's'; return 'a';}}, {toString(){log += 'r'; return 'X';}}); r === 'Xba' && log === 'tsr'"#,
        r#"String.prototype.replace.call(123, 2, 4) === '143' && 'undefined'.replace() === 'undefined'"#,
        r#"let d = Object.getOwnPropertyDescriptor(String.prototype, 'replaceAll'); d.value.length === 2 && d.value.name === 'replaceAll' && d.writable && !d.enumerable && d.configurable && String.prototype.replace.length === 2"#,
        r#"let caught = false; try { String.prototype.replace.call(null, '', 'x'); } catch(e) { caught = e instanceof TypeError; } caught"#,
        r#"let caught = false; try { 'x'.replace(Symbol(), ''); } catch(e) { caught = e instanceof TypeError; } caught"#,
        r#"let r = /a/g; Object.defineProperty(r, 'lastIndex', {writable:false}); let caught = false; try { 'a'.replace(r, 'b'); } catch(e) { caught = e instanceof TypeError; } caught"#,
        // The documented brand-dispatch policy ignores overridden exec/flags.
        r#"let r = /a/g; r.exec = () => {throw 1;}; Object.defineProperty(r,'flags',{get(){throw 2;}}); 'aa'.replaceAll(r, 'b') === 'bb'"#,
    ] {
        assert_eq!(eval(source), ValueView::Bool(true), "{source}");
    }
}

/// Synthetic fixtures, not light-cljs output. Saved arguments, replacer
/// closures, groups and callback results remain live across every boundary.
#[test]
fn string_replace_wait_yield_collect_and_migrate() {
    for expression in [
        "'🐱'.replace(/(?:)/gu, (m,p) => {if(p === 2){count++; __rt.need(189,1);} return '-';}) === '-🐱-'",
        "String.prototype.replace.call({get toString(){count++; __rt.need(189,1); return () => 'abc';}}, 'b', 'X') === 'aXc'",
        "'abc'.replace({toString(){count++; __rt.need(189,1); return 'b';}}, 'X') === 'aXc'",
        "'abc'.replaceAll('b', {toString(){count++; __rt.need(189,1); return 'X';}}) === 'aXc'",
        "let r = /b/; r.lastIndex = {valueOf(){count++; __rt.need(189,1); return 0;}};| 'abc'.replace(r,'X') === 'aXc'",
        "let r = /b/; r.lastIndex = {valueOf(){count++; __rt.need(189,1); return ''.padEnd(65537,'0');}}; let caught = false; try { 'abc'.replace(r,'X'); } catch(e) { caught = e instanceof RangeError && e.message.indexOf('Number conversion limit') >= 0; }| caught",
        "let captured = {text:'X'};| 'abc'.replace(/(?<x>b)(z)?/, (m,b,z,p,s,g) => { count++; __rt.need(189,1); return captured.text + g.x + p + s + String(z); }) === 'aXb1abcundefinedc'",
        "'abc'.replaceAll('b', () => ({get toString(){count++; __rt.need(189,1); return () => 'X';}})) === 'aXc'",
        "let caught = false; try { 'abc'.replace(/b/, () => { count++; __rt.need(189,1); throw 42; }); } catch(e) { caught = e === 42; }| caught",
    ] {
        let rt = Runtime::new();
        let (setup, result) = expression.split_once('|').unwrap_or(("", expression));
        let source = format!(
            "let count = 0; let result = (() => {{ {setup} return {result}; }})(); result && count === 1"
        );
        let mut task = rt.spawn(&exe(&source), &[]).unwrap();
        let mut waits = 0;
        let mut done = false;
        for _ in 0..1500 {
            let (next, outcome) = std::thread::spawn(move || {
                let outcome = task
                    .run_with(Budget::instructions(1), &mut NoEffects)
                    .unwrap();
                (task, outcome)
            })
            .join()
            .unwrap();
            task = next;
            rt.collect().unwrap();
            match outcome {
                RunOutcome::Yielded => {}
                RunOutcome::Waiting(_) => {
                    waits += 1;
                    assert_eq!(waits, 1, "callback replay: {source}");
                    let request = task.take_producer_requests().pop().unwrap();
                    let value = rt.root(Primitive::Number(0.0));
                    task = rt
                        .complete(request.key(), request.producer(), Completion::Ready(&value))
                        .unwrap()
                        .into_runnable()
                        .pop()
                        .unwrap();
                }
                RunOutcome::Complete(v) => {
                    assert_eq!(v.view(), ValueView::Bool(true), "{source}");
                    done = true;
                    break;
                }
                other => panic!("{source}: {other:?}"),
            }
        }
        assert!(done, "instruction bound: {source}");
        assert_eq!(waits, 1);
    }
}

/// Synthetic fixtures, not light-cljs output. Check limits before flattening
/// arguments, and bound aggregate substitution/capture work across steps.
#[test]
fn string_replace_limits_and_budgeted_scans() {
    for source in [
        "let s = ''.padEnd(65537, 'a'); let caught = false; try { s.replace('a','b'); } catch(e) { caught = e instanceof RangeError; } caught",
        "let s = ''.padEnd(65537, 'a'); let caught = false; try { 'a'.replace(s,'b'); } catch(e) { caught = e instanceof RangeError; } caught",
        "let s = ''.padEnd(65537, 'a'); let caught = false; try { 'a'.replace('a',s); } catch(e) { caught = e instanceof RangeError; } caught",
        "let s = ''.padEnd(65537, 'a'); let caught = false; try { 'a'.replace('a',() => s); } catch(e) { caught = e instanceof RangeError; } caught",
        "let s = ''.padEnd(2000, 'a'); let caught = false; try { s.replaceAll('',s); } catch(e) { caught = e instanceof RangeError && e.message.indexOf('work limit') >= 0; } caught",
        "let caught = false; try { (''.padEnd(100,'a') + 'b').replace(/^(a+)+$/, 'x'); } catch(e) { caught = e instanceof RangeError && e.message.indexOf('regular expression step limit') >= 0; } caught",
    ] {
        assert_eq!(eval(source), ValueView::Bool(true), "{source}");
    }
    let rt = Runtime::new();
    let input = rt.root(Primitive::String(
        ("a".repeat(65_535) + "b").as_str().into(),
    ));
    let code = compile_function(
        "function replace(s) { return s.replace('b', 'X').charAt(65535) === 'X'; }",
    )
    .unwrap();
    let task = rt.spawn(&code, &[&input]).unwrap();
    let mut yields = 0;
    for _ in 0..2000 {
        match task
            .run_with(Budget::instructions(1), &mut NoEffects)
            .unwrap()
        {
            RunOutcome::Yielded => yields += 1,
            RunOutcome::Complete(v) => {
                assert_eq!(v.view(), ValueView::Bool(true));
                assert!(yields > 200, "long scan must yield between bounded batches");
                return;
            }
            other => panic!("replacement scan: {other:?}"),
        }
    }
    panic!("replacement scan exceeded instruction bound");
}

/// Synthetic fixtures, not light-cljs output. The limit is applied after
/// resumable ToPrimitive and before leaf StringToNumber scans or flattens.
#[test]
fn string_replace_last_index_conversion_is_bounded() {
    for source in [
        "let r = /b/; r.lastIndex = ''.padEnd(65537,'0'); let caught = false; try { 'abc'.replace(r,'X'); } catch(e) { caught = e instanceof RangeError && e.message.indexOf('Number conversion limit of 65536') >= 0; } caught",
        "let r = /b/y; r.lastIndex = {valueOf(){return {};}, toString(){return ''.padEnd(65537,'0');}}; let caught = false; try { 'abc'.replace(r,'X'); } catch(e) { caught = e instanceof RangeError; } caught",
        "let r = /b/; r.lastIndex = ''.padEnd(65536,'0'); 'abc'.replace(r,'X') === 'aXc'",
        "let r = /b/g; r.lastIndex = ''.padEnd(65537,'0'); 'abc'.replace(r,'X') === 'aXc' && r.lastIndex === 0",
    ] {
        assert_eq!(eval(source), ValueView::Bool(true), "{source}");
    }
}

/// Synthetic fixtures, not light-cljs output. Capture ranges are charged
/// during collection, before any replacer can observe a partial result.
#[test]
fn string_replace_capture_storage_limit() {
    for source in [
        "let r = new RegExp(''.padEnd(1000, '()'), 'g'); let calls = 0; let caught = false; try { ''.padEnd(2000, 'a').replace(r, () => {calls++; return '';}); } catch(e) { caught = e instanceof RangeError && e.message.indexOf('string replacement work limit') >= 0; } caught && calls === 0",
        "let r = new RegExp(''.padEnd(1000, '()'), 'g'); let caught = false; try { ''.padEnd(2000, 'a').replaceAll(r, ''); } catch(e) { caught = e instanceof RangeError && e.message.indexOf('string replacement work limit') >= 0; } caught",
        "let r = new RegExp(''.padEnd(1000, '()'), 'g'); let calls = 0; let text = ''.padEnd(10, 'a'); text.replace(r, () => {calls++; return '';}) === text && calls === 11 && r.lastIndex === 0",
    ] {
        assert_eq!(eval(source), ValueView::Bool(true), "{source}");
    }
}

/// Synthetic fixtures, not light-cljs output. The first two rows adapt
/// research/quickjs/tests/test_bigint.js::test_bigint3 at 7d1c13c5d8f8.
/// Copyright (c) 2017-2021 Fabrice Bellard and Charlie Gordon.
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
fn bigint_to_number_conversion() {
    for src in [
        "Number(1n) === 1 && new Number(1n).valueOf() === 1",
        "Number({ valueOf() { return 1n; } }) === 1 && new Number({ valueOf() { return 1n; } }).valueOf() === 1",
        "let a = 0; let b = 0; let c = 0; try { a = Number(1n); } catch(e) { a = 99; } try { b = new Number(1n).valueOf(); } catch(e) { b = 99; } try { c = Number({valueOf() { return 1n; }}); } catch(e) { c = 99; } a === 1 && b === 1 && c === 1",
        "Number(0xffffffffffffffffn) === 18446744073709552000",
        "Number(-0xffffffffffffffffn) === -18446744073709552000",
        "Number(9007199254740993n) === 9007199254740992",
        "Number(9007199254740995n) === 9007199254740996",
        "Number(18446744073709551615n) === 18446744073709551616",
        "Number(2n ** 1024n) === Infinity && Number(-(2n ** 1024n)) === -Infinity",
        "Number(0n) === 0 && 1 / Number(0n) === Infinity && 1 / Number(-0n) === Infinity",
        "Number(123n) === 123 && Number(-123n) === -123",
        "Number(-9007199254740993n) === -9007199254740992 && Number(-9007199254740995n) === -9007199254740996",
        "Number(9223372036854775807n) === 9223372036854775808 && Number(-9223372036854775808n) === -9223372036854775808",
        "Number((1n << 100n) + (1n << 47n)) === 2 ** 100",
        "Number((1n << 100n) + (1n << 47n) + 1n) === 2 ** 100 + 2 ** 48",
        "Number((1n << 100n) + (1n << 47n) - 1n) === 2 ** 100",
        "Number((1n << 100n) + (3n << 47n)) === 2 ** 100 + 2 ** 49",
        "Number((1n << 100n) - (1n << 46n)) === 2 ** 100",
        "Number((1n << 100n) - (1n << 46n) - 1n) === 2 ** 100 - 2 ** 47",
        "let max = ((1n << 53n) - 1n) << 971n; Number(max) === Number.MAX_VALUE && Number(max + (1n << 970n) - 1n) === Number.MAX_VALUE",
        "let boundary = (1n << 1024n) - (1n << 970n); Number(boundary) === Infinity && Number(-boundary) === -Infinity && Number(-boundary + 1n) === -Number.MAX_VALUE",
        "Number(1n << 10000n) === Infinity && Number(-(1n << 10000n)) === -Infinity",
        "new Number(9007199254740995n).valueOf() === 9007199254740996",
        "let calls = 0; let o = {valueOf() { calls++; return 9007199254740993n; }, toString() { throw 99; }}; Number(o) === 9007199254740992 && new Number(o).valueOf() === 9007199254740992 && calls === 2",
        "let calls = 0; Number({valueOf() { calls++; return {}; }, toString() { calls++; return 123n; }}) === 123 && calls === 2",
        "Math.ceil(Number(123n) / 10) === 13",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
}

/// Synthetic fixtures, not light-cljs output. Every currently implemented
/// Math operation, Number branding/predicates, Date numeric entry point and
/// typed-array storage kind preserves the implicit ToNumber rejection.
#[test]
fn bigint_number_only_intrinsics_reject_implicit_conversion() {
    let mut expressions = vec![
        "+1n".to_owned(),
        "isNaN(1n)".to_owned(),
        "isFinite(1n)".to_owned(),
        "Math.pow(1, 1n)".to_owned(),
        "Math.imul(1, 1n)".to_owned(),
        "Math.hypot(Infinity, 1n)".to_owned(),
        "Number.prototype.toString.call(1n)".to_owned(),
        "Number.prototype.valueOf.call(1n)".to_owned(),
        "(123).toString(1n)".to_owned(),
        "new Number(123).toString({valueOf() { return 1n; }})".to_owned(),
        "new Date(1n)".to_owned(),
        "new Date(2000, 1n)".to_owned(),
        "Date.UTC(2000, 1n)".to_owned(),
        "new Date(0).setTime(1n)".to_owned(),
        "new Date(0).setUTCHours(1, 1n)".to_owned(),
        "new Date(0).setHours(1, 1n)".to_owned(),
        "new ArrayBuffer(1n)".to_owned(),
        "new ArrayBuffer(8).slice(1n)".to_owned(),
        "new ArrayBuffer(8).transfer(1n)".to_owned(),
        "String.fromCharCode(1n)".to_owned(),
        "String.fromCodePoint(1n)".to_owned(),
        "'a'.charAt(1n)".to_owned(),
        "'a'.charCodeAt(1n)".to_owned(),
        "'a'.codePointAt(1n)".to_owned(),
        "'a'.indexOf('a', 1n)".to_owned(),
        "'a'.lastIndexOf('a', 1n)".to_owned(),
        "'a'.substring(1n)".to_owned(),
        "'a'.slice(1n)".to_owned(),
        "'a'.padStart(1n)".to_owned(),
        "'a'.padEnd(1n)".to_owned(),
        "'a'.split('', 1n)".to_owned(),
        "parseInt('10', 1n)".to_owned(),
        "Number.parseInt('10', 1n)".to_owned(),
        "BigInt.prototype.toString.call(123n, 1n)".to_owned(),
        "let a = []; a.length = 1n".to_owned(),
        "Object.defineProperty([], 'length', {value: 1n})".to_owned(),
        "Array.prototype.push.call({length: 1n}, 0)".to_owned(),
        "Array.prototype.join.call({length: 1n})".to_owned(),
        "Array.prototype.toLocaleString.call({length: 1n})".to_owned(),
        "(function() {}).apply(null, {length: 1n})".to_owned(),
        "Reflect.apply(function() {}, null, {length: 1n})".to_owned(),
        "String.raw({raw: {length: 1n}})".to_owned(),
        "Intl.getCanonicalLocales({length: 1n})".to_owned(),
        "let r = /a/g; r.lastIndex = 1n; r.exec('a')".to_owned(),
        "let r = /a/y; r.lastIndex = 1n; r.test('a')".to_owned(),
        "let r = /a/y; r.lastIndex = 1n; 'a'.replace(r, '')".to_owned(),
    ];
    for name in [
        "floor", "ceil", "abs", "fround", "sqrt", "log2", "pow", "imul", "hypot",
    ] {
        expressions.push(format!("Math.{name}(1n)"));
        expressions.push(format!("Math.{name}({{valueOf() {{ return 1n; }} }})"));
    }
    for name in [
        "setUTCFullYear",
        "setUTCMonth",
        "setUTCDate",
        "setUTCHours",
        "setUTCMinutes",
        "setUTCSeconds",
        "setUTCMilliseconds",
        "setFullYear",
        "setMonth",
        "setDate",
        "setHours",
        "setMinutes",
        "setSeconds",
        "setMilliseconds",
    ] {
        expressions.push(format!("new Date(0).{name}(1n)"));
        expressions.push(format!(
            "new Date(NaN).{name}({{valueOf() {{ return 1n; }} }})"
        ));
    }
    for name in [
        "Int8Array",
        "Uint8Array",
        "Uint8ClampedArray",
        "Int16Array",
        "Uint16Array",
        "Int32Array",
        "Uint32Array",
        "Float32Array",
        "Float64Array",
    ] {
        for body in [
            "new TYPE(1n)",
            "new TYPE({length: 1n})",
            "new TYPE(new ArrayBuffer(8), 1n)",
            "new TYPE([1n])",
            "new TYPE(1)[0] = 1n",
            "new TYPE(1)[0] = {valueOf() { return 1n; }}",
            "new TYPE(1).set([1n])",
            "new TYPE(1).set([], 1n)",
            "new TYPE(1).fill(1n)",
            "new TYPE(1).fill(0, 1n)",
            "new TYPE(1).fill(0, 0, 1n)",
        ] {
            expressions.push(body.replace("TYPE", name));
        }
    }
    for expression in expressions {
        let src = format!(
            "let caught = false; try {{ {expression}; }} catch (e) {{ caught = e instanceof TypeError; }} caught"
        );
        assert_eq!(eval(&src), ValueView::Bool(true), "{expression}");
    }
    for src in [
        "!Number.isNaN(1n) && !Number.isFinite(1n) && !Number.isInteger(1n) && !Number.isSafeInteger(1n)",
        "let calls = 0; let o = {valueOf() { calls++; return 1n; }}; !Number.isNaN(o) && !Number.isFinite(o) && !Number.isInteger(o) && !Number.isSafeInteger(o) && calls === 0",
        "Math.floor(1, 2n) === 1 && isNaN(NaN, 1n) && isFinite(1, 1n)",
        "parseInt(123n) === 123 && parseFloat(123n) === 123",
        "let order = ''; let caught = false; try { Math.pow({valueOf() { order += 'a'; return 1n; }}, {valueOf() { order += 'b'; return 2; }}); } catch (e) { caught = e instanceof TypeError; } caught && order === 'a'",
        "let d = new Date(0); let order = ''; let caught = false; try { d.setUTCHours({valueOf() { order += 'a'; return 1n; }}, {valueOf() { order += 'b'; return 2; }}); } catch (e) { caught = e instanceof TypeError; } caught && order === 'a' && d.getTime() === 0",
        "let d = new Date(0); let order = ''; let caught = false; try { d.setHours({valueOf() { order += 'a'; return 1n; }}, {valueOf() { order += 'b'; return 2; }}); } catch (e) { caught = e instanceof TypeError; } caught && order === 'a' && d.getTime() === 0",
        "let a = new Uint8Array([7]); let caught = false; try { a[0] = 1n; } catch(e) { caught = e instanceof TypeError; } caught && a[0] === 7",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
}

/// Synthetic fixtures, not light-cljs output. Number's explicit conversion
/// and implicit rejection retain ordinary continuations across callback waits.
#[test]
fn bigint_number_conversion_waits_collects_and_migrates() {
    for expression in [
        "Number(o) === 9007199254740996",
        "new Number(o).valueOf() === 9007199254740996",
        "(() => { try { Math.floor(o); } catch(e) { return e instanceof TypeError; } return false; })()",
        "(() => { try { isNaN(o); } catch(e) { return e instanceof TypeError; } return false; })()",
        "(() => { try { new Uint8Array(1)[0] = o; } catch(e) { return e instanceof TypeError; } return false; })()",
    ] {
        let rt = Runtime::new();
        let source = format!(
            "let calls = 0; let o = {{get valueOf() {{ calls++; __rt.need(2031, 1); return function() {{ calls++; __rt.need(2032, 1); return 9007199254740995n; }}; }}, toString() {{ throw 99; }} }}; let result = {expression}; result && calls === 2"
        );
        let mut task = rt.spawn(&exe(&source), &[]).unwrap();
        let mut waits = 0;
        let mut completed = false;
        for _ in 0..1000 {
            let (next, outcome) = std::thread::spawn(move || {
                let outcome = task
                    .run_with(Budget::instructions(1), &mut NoEffects)
                    .unwrap();
                (task, outcome)
            })
            .join()
            .unwrap();
            task = next;
            match outcome {
                RunOutcome::Yielded => {
                    rt.collect().unwrap();
                }
                RunOutcome::Waiting(_) => {
                    waits += 1;
                    assert!(waits <= 2, "{expression}");
                    rt.collect().unwrap();
                    let request = task.take_producer_requests().pop().unwrap();
                    let value = rt.root(Primitive::Null);
                    task = rt
                        .complete(request.key(), request.producer(), Completion::Ready(&value))
                        .unwrap()
                        .into_runnable()
                        .pop()
                        .unwrap();
                }
                RunOutcome::Complete(value) => {
                    assert_eq!(value.view(), ValueView::Bool(true), "{expression}");
                    assert_eq!(waits, 2);
                    completed = true;
                    break;
                }
                other => panic!("{expression}: {other:?}"),
            }
        }
        assert!(completed, "{expression}");
    }
}

/*
QuickJS Javascript Engine

Copyright (c) 2017-2021 Fabrice Bellard
Copyright (c) 2017-2021 Charlie Gordon

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in
all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL
THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
THE SOFTWARE.
*/
/// Synthetic fixtures, not light-cljs output. Decimal examples follow
/// research/quickjs/tests/test_intl.js (7d1c13c5d8f8); see docs/attribution.md.
#[test]
fn intl_number_format_decimal() {
    for (locale, expected) in [
        ("en", "1,234,567.89"),
        ("de", "1.234.567,89"),
        ("fr", "1\u{202f}234\u{202f}567,89"),
        ("es", "1.234.567,89"),
        ("ja", "1,234,567.89"),
        ("zh", "1,234,567.89"),
        ("ar", "1,234,567.89"),
        ("ru", "1\u{a0}234\u{a0}567,89"),
        ("pt-BR", "1.234.567,89"),
    ] {
        assert_eq!(
            eval(&format!(
                "new Intl.NumberFormat('{locale}').format(1234567.89) === '{expected}'"
            )),
            ValueView::Bool(true)
        );
    }
    for source in [
        "new Intl.NumberFormat('en').format(1000000) === '1,000,000'",
        "new Intl.NumberFormat('de').format(1000000) === '1.000.000'",
        "new Intl.NumberFormat('fr').format(1000000) === '1\u{202f}000\u{202f}000'",
        "let f = new Intl.NumberFormat('en').format; f(0.1234567) === '0.123' && f(-0) === '-0' && f(NaN) === 'NaN' && f(Infinity) === '∞'",
        "new Intl.NumberFormat('en', {useGrouping:false}).format(1234) === '1234'",
        "new Intl.NumberFormat('en', {maximumFractionDigits:1}).format(2.25) === '2.3'",
        "let n = new Intl.NumberFormat('de'); let f = n.format; f(1.5) === '1,5' && f === n.format && f !== new Intl.NumberFormat('de').format",
        "let caught = false; try { new Intl.NumberFormat('en', {maximumFractionDigits:101}); } catch(e) {caught = e instanceof RangeError;} caught",
    ] {
        assert_eq!(eval(source), ValueView::Bool(true), "{source}");
    }
}

/// Synthetic fixtures, not light-cljs output.
#[test]
fn intl_number_format_options_and_rounding() {
    for source in [
        "let o=Intl.NumberFormat().resolvedOptions(); o.locale === 'en-US' && o.numberingSystem === 'latn' && o.style === 'decimal' && o.minimumIntegerDigits === 1 && o.minimumFractionDigits === 0 && o.maximumFractionDigits === 3 && o.useGrouping === 'auto' && o.roundingMode === 'halfExpand' && o.notation === 'standard'",
        "let o=new Intl.NumberFormat('EN-us-u-nu-latn').resolvedOptions(); o.locale === 'en-US-u-nu-latn'",
        "Intl.NumberFormat.supportedLocalesOf(['xx','DE-de','en-US-u-nu-latn','pt','pt-BR','de-DE']).join() === 'de-DE,en-US-u-nu-latn,pt-BR'",
        "let n=new Intl.NumberFormat('en',{minimumIntegerDigits:5,minimumFractionDigits:4}); n.format(1.2) === '00,001.2000' && n.resolvedOptions().maximumFractionDigits === 4",
        "let n=new Intl.NumberFormat('en',{useGrouping:'min2'}); n.format(1234) === '1234' && n.format(12345) === '12,345' && n.format(1234567) === '1,234,567'",
        "new Intl.NumberFormat('es',{useGrouping:'auto'}).format(1234.5) === '1.234,5'",
        "let n=new Intl.NumberFormat('en',{maximumFractionDigits:2}); n.format(1.005) === '1.01' && n.format(9.999) === '10' && n.format(-0.0001) === '-0' && n.format(1e21) === '1,000,000,000,000,000,000,000'",
        "let n=new Intl.NumberFormat('en',{minimumSignificantDigits:3,maximumSignificantDigits:3}); n.format(0.00123456) === '0.00123' && n.format(999.9) === '1,000' && n.format(9.999) === '10.0' && n.format(1) === '1.00' && n.format(0) === '0.00' && n.resolvedOptions().maximumSignificantDigits === 3 && n.resolvedOptions().maximumFractionDigits === 3",
        "let n=new Intl.NumberFormat('en',{useGrouping:false}); n.format(123456789012345678901234567890n) === '123456789012345678901234567890' && n.format('123456789012345678901234567890.125') === '123456789012345678901234567890.125'",
        "new Intl.NumberFormat('de',{minimumFractionDigits:2}).format(-12345678901234567890n) === '-12.345.678.901.234.567.890,00'",
        "let n=new Intl.NumberFormat('ar'); n.format(-1) === '\u{200e}-1' && n.format(NaN) === 'ليس\u{00a0}رقمًا'",
        "let n=new Intl.NumberFormat('ru'); n.format(NaN) === 'не\u{00a0}число'",
        "let n=new Intl.NumberFormat('en'); n[Symbol.toStringTag] === 'Intl.NumberFormat' && n instanceof Intl.NumberFormat && n.format.length === 1 && n.format.name === '' && n.format.call({},1.5) === '1.5'",
        "let g=Object.getOwnPropertyDescriptor(Intl.NumberFormat.prototype,'format'); !g.enumerable && g.configurable && g.set === undefined && g.get.name === 'get format' && g.get.length === 0 && Intl.NumberFormat.name === 'NumberFormat' && Intl.NumberFormat.length === 1 && Intl.NumberFormat.supportedLocalesOf.length === 1",
        "let n=new Intl.NumberFormat('en'); Object.freeze(n); n.format === n.format && n.format(1.5) === '1.5'",
        "let n=new Intl.NumberFormat('en',{useGrouping:''}); n.format(1234) === '1234'",
        "let caught=false; let later=false; let finalized=false; try { new Intl.NumberFormat('en',{style:1n << 1000n,get notation(){later=true;return 'standard';}}); } catch(e) { caught=e instanceof RangeError; } finally { finalized=true; } caught && !later && finalized",
        "let caught=false; try { new Intl.NumberFormat('en',{maximumFractionDigits:1n << 1000n}); } catch(e) { caught=e instanceof TypeError; } caught",
    ] {
        assert_eq!(eval(source), ValueView::Bool(true), "{source}");
    }
    for (mode, positive, negative) in [
        ("ceil", "2.3", "-2.2"),
        ("floor", "2.2", "-2.3"),
        ("expand", "2.3", "-2.3"),
        ("trunc", "2.2", "-2.2"),
        ("halfCeil", "2.3", "-2.2"),
        ("halfFloor", "2.2", "-2.3"),
        ("halfExpand", "2.3", "-2.3"),
        ("halfTrunc", "2.2", "-2.2"),
        ("halfEven", "2.2", "-2.2"),
    ] {
        let source = format!(
            "let n=new Intl.NumberFormat('en',{{maximumFractionDigits:1,roundingMode:'{mode}'}}); n.format(2.25) === '{positive}' && n.format(-2.25) === '{negative}'"
        );
        assert_eq!(eval(&source), ValueView::Bool(true), "{source}");
    }
    for option in [
        "minimumIntegerDigits:0",
        "minimumIntegerDigits:22",
        "minimumFractionDigits:-1",
        "maximumFractionDigits:101",
        "maximumFractionDigits:NaN",
        "minimumFractionDigits:2,maximumFractionDigits:1",
        "minimumSignificantDigits:0",
        "maximumSignificantDigits:22",
        "minimumSignificantDigits:4,maximumSignificantDigits:3",
        "notation:'scientific'",
        "notation:'compact'",
        "roundingMode:'unknown'",
        "localeMatcher:'unknown'",
        "numberingSystem:'bad!'",
        "useGrouping:'never'",
        "useGrouping:1",
        "style:'unit'",
    ] {
        let source = format!("new Intl.NumberFormat('en',{{{option}}})");
        assert_eq!(failure(&source).kind(), VmErrorKind::RangeError, "{source}");
    }
    for source in [
        "new Intl.NumberFormat('en',{style:'currency'})",
        "new Intl.NumberFormat('en',null)",
        "Intl.NumberFormat.prototype.resolvedOptions.call({})",
        "Object.getOwnPropertyDescriptor(Intl.NumberFormat.prototype,'format').get.call({})",
        "new Intl.NumberFormat('en').format(Symbol())",
        "new Intl.NumberFormat.supportedLocalesOf('en')",
        "Intl.NumberFormat.supportedLocalesOf(null)",
    ] {
        assert_eq!(failure(source).kind(), VmErrorKind::TypeError, "{source}");
    }
}

/// Synthetic fixture, not light-cljs output. Saved options, locale-list and
/// numeric coercions survive collection at each yield and worker migration.
#[test]
fn intl_number_format_callbacks_wait_collect_and_migrate() {
    let rt = Runtime::new();
    let source = r#"
        let log='';
        let n=new Intl.NumberFormat({get length(){log+='l';__rt.need(1,1);return 1;},get 0(){log+='t';__rt.need(2,1);return 'DE';}},
            {get maximumFractionDigits(){log+='f';__rt.need(3,1);return {valueOf(){log+='v';__rt.need(4,1);return 1;}};},get useGrouping(){log+='g';__rt.need(5,1);return false;}});
        let f=n.format; n=null;
        let text=f({valueOf(){log+='n';__rt.need(6,1);return 2.25;}});
        let big=f({valueOf(){log+='b';__rt.need(7,1);return 12345678901234567890n;}});
        text === '2,3' && big === '12345678901234567890' && log === 'ltfvgnb'
    "#;
    let mut task = rt.spawn(&exe(source), &[]).unwrap();
    let mut waited = 0;
    for _ in 0..2000 {
        let (next, outcome) = std::thread::spawn(move || {
            let outcome = task
                .run_with(Budget::instructions(1), &mut NoEffects)
                .unwrap();
            (task, outcome)
        })
        .join()
        .unwrap();
        task = next;
        match outcome {
            RunOutcome::Yielded => {
                rt.collect().unwrap();
            }
            RunOutcome::Waiting(_) => {
                waited += 1;
                assert!(waited <= 7);
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
            RunOutcome::Complete(value) => {
                assert_eq!(value.view(), ValueView::Bool(true));
                assert_eq!(waited, 7);
                return;
            }
            other => panic!("{other:?}"),
        }
    }
    panic!("NumberFormat continuation did not finish");
}

/// Synthetic fixture, not light-cljs output.
#[test]
fn intl_number_format_host_default_and_ownership() {
    struct LocaleHost(&'static str, usize);
    impl Host for LocaleHost {
        fn effect(&mut self, _: HostCall) -> Result<Primitive, String> {
            unreachable!()
        }
        fn default_locale(&mut self) -> String {
            self.1 += 1;
            self.0.into()
        }
    }
    let rt = Runtime::new();
    let task=rt.spawn(&exe("let n=new Intl.NumberFormat(); __rt.need(1,1); n.format(1.5) === '1,5' && n.resolvedOptions().locale === 'de'"),&[]).unwrap();
    let mut host = LocaleHost("de", 0);
    assert!(matches!(
        task.run_with(Budget::instructions(10000), &mut host)
            .unwrap(),
        RunOutcome::Waiting(_)
    ));
    assert_eq!(host.1, 1);
    host.0 = "fr";
    let request = task.take_producer_requests().pop().unwrap();
    let zero = rt.root(Primitive::Number(0.0));
    let task = rt
        .complete(request.key(), request.producer(), Completion::Ready(&zero))
        .unwrap()
        .into_runnable()
        .pop()
        .unwrap();
    assert!(
        matches!(task.run_with(Budget::instructions(10000),&mut host).unwrap(),RunOutcome::Complete(value) if value.view()==ValueView::Bool(true))
    );
    assert_eq!(host.1, 1);
    for source in [
        "new Intl.NumberFormat('en')",
        "new Intl.NumberFormat('en').format",
    ] {
        let owner = rt.spawn(&exe(source), &[]).unwrap();
        let RunOutcome::Complete(value) = owner.run().unwrap() else {
            panic!("expected formatter")
        };
        assert!(matches!(
            rt.publish(&value),
            Err(clj_quickjs::PublishError::NotShareable { .. })
        ));
        let body = if source.ends_with(".format") {
            "value(1)"
        } else {
            "Intl.NumberFormat.prototype.resolvedOptions.call(value)"
        };
        let code = compile_function(&format!("function f(value){{return {body};}}")).unwrap();
        let caller = rt.spawn(&code, &[&value]).unwrap();
        assert!(
            matches!(caller.run().unwrap(),RunOutcome::Failed(error) if matches!(error.kind(),VmErrorKind::ForeignObject{..}))
        );
    }
    rt.collect().unwrap();
}

/// Synthetic fixtures, not light-cljs output. Number systems use the pinned
/// QuickJS digit mapping while keeping each locale's separators and symbols.
#[test]
fn intl_number_format_numbering_systems() {
    for source in [
        "let n=new Intl.NumberFormat('en-u-nu-arab'); n.format(1234.5) === '١,٢٣٤.٥' && n.resolvedOptions().numberingSystem === 'arab' && n.resolvedOptions().locale === 'en-u-nu-arab'",
        "let n=new Intl.NumberFormat('en-u-nu-arab',{numberingSystem:'latn'}); n.format(1234) === '1,234' && n.resolvedOptions().locale === 'en' && n.resolvedOptions().numberingSystem === 'latn'",
        "let n=new Intl.NumberFormat('de',{numberingSystem:'fullwide'}); n.format(-1234.5) === '-１.２３４,５' && n.resolvedOptions().numberingSystem === 'fullwide'",
        "new Intl.NumberFormat('en',{numberingSystem:'hanidec'}).format(1234567890) === '一,二三四,五六七,八九〇'",
        "new Intl.NumberFormat('en',{numberingSystem:'arabext'}).format(1234.5) === '۱,۲۳۴.۵'",
        "new Intl.NumberFormat('en',{numberingSystem:'mathbold',useGrouping:false}).format(12n) === '𝟏𝟐'",
        "let n=new Intl.NumberFormat('en-u-nu-arab',{numberingSystem:'notreal'}); n.format(123) === '١٢٣' && n.resolvedOptions().numberingSystem === 'arab'",
        "let n=new Intl.NumberFormat('en',{numberingSystem:'roman'}); n.format(1234) === '1,234' && n.resolvedOptions().numberingSystem === 'roman'",
    ] {
        assert_eq!(eval(source), ValueView::Bool(true), "{source}");
    }
}

/// Synthetic fixtures, not light-cljs output. Invalid significant digit bounds
/// fail before later getters can throw, wait, or otherwise become observable.
#[test]
fn intl_number_format_invalid_bounds_stop_later_getters() {
    for later in ["roundingMode", "useGrouping"] {
        let source = format!(
            "let log=''; let caught=false; try {{ new Intl.NumberFormat('en',{{minimumSignificantDigits:4,maximumSignificantDigits:3,get {later}(){{log+='x'; throw 7;}}}}); }} catch(e){{caught=e instanceof RangeError;}} caught && log === ''"
        );
        assert_eq!(eval(&source), ValueView::Bool(true), "{source}");
        let source = format!(
            "let caught=false; try {{ new Intl.NumberFormat('en',{{minimumSignificantDigits:4,maximumSignificantDigits:3,get {later}(){{__rt.need(900,1);return 'auto';}}}}); }} catch(e){{caught=e instanceof RangeError;}} caught"
        );
        assert_eq!(eval(&source), ValueView::Bool(true), "{source}");
    }
}

/// Synthetic fixtures, not light-cljs output. All valid oversized exponents
/// take the documented resource failure, independent of integer parse width.
#[test]
fn intl_number_format_decimal_resource_limits() {
    for text in [
        "1e10001",
        "1e-10001",
        "1e2147483648",
        "1e-2147483649",
        "1e999999999999999999999",
        "1e-999999999999999999999",
    ] {
        let source = format!("new Intl.NumberFormat('en').format('{text}')");
        assert_eq!(failure(&source).kind(), VmErrorKind::RangeError, "{source}");
    }
    for text in [
        "1e",
        "1e+",
        "1e-",
        "1ex",
        "1e999999999999999999999x",
        "1e-10001x",
        "xe10001",
        "1..0e-10001",
        "e10001",
        ".e10001",
        "xE+10001",
        "1x0e999999999999999999999",
        ".e-999999999999999999999",
        "\u{0085}1",
        "\u{0085}1e10001",
        "1e-10001\u{0085}",
    ] {
        assert_eq!(
            eval(&format!(
                "new Intl.NumberFormat('en').format('{text}') === 'NaN'"
            )),
            ValueView::Bool(true),
            "{text}"
        );
    }
    for mantissa in [
        "1".repeat(100_000) + "x",
        "1".repeat(100_000) + "..",
        ".".repeat(100_001),
    ] {
        for exponent in ["", "e10001", "e-10001"] {
            assert_eq!(
                eval(&format!(
                    "new Intl.NumberFormat('en').format('{mantissa}{exponent}') === 'NaN'"
                )),
                ValueView::Bool(true)
            );
        }
    }
    let oversized = "1".repeat(100_001);
    assert_eq!(
        failure(&format!(
            "new Intl.NumberFormat('en').format('{oversized}')"
        ))
        .kind(),
        VmErrorKind::RangeError
    );
    let boundary = "0".repeat(99_999) + "1";
    assert_eq!(
        eval(&format!(
            "new Intl.NumberFormat('en').format('{boundary}') === '1'"
        )),
        ValueView::Bool(true)
    );
    assert_eq!(
        eval(
            "let f=new Intl.NumberFormat('en',{useGrouping:false}).format; let s=f('1e10000'); s.length === 10001 && s[0] === '1' && s[10000] === '0' && f('1e-10000') === '0' && f('1e+00000000000000000000001') === '10' && f('\u{feff}\u{00a0}1\u{2028}\u{2029}') === '1'"
        ),
        ValueView::Bool(true)
    );
}

/// Synthetic fixtures, not light-cljs output. BigInt decimal conversion must
/// keep yielding after argument evaluation, including after object conversion.
#[test]
fn intl_number_format_large_bigint_yields_collects_and_migrates() {
    let digits: String = (0..4000)
        .map(|i| char::from(b'0' + ((i * 7 + 1) % 10) as u8))
        .collect();
    for (argument, calls) in [
        ("__rt.need(2131,1)", 0),
        ("{valueOf(){calls++;return __rt.need(2131,1);}}", 1),
    ] {
        let rt = Runtime::new();
        let source = format!(
            "let calls=0; let f=new Intl.NumberFormat('en',{{useGrouping:false}}).format; let result=f({argument}); result === '{digits}' && calls === {calls}"
        );
        let mut task = rt.spawn(&exe(&source), &[]).unwrap();
        let mut waits = 0;
        let mut conversion_yields = 0;
        let mut completed = false;
        for _ in 0..2000 {
            let (next, outcome) = std::thread::spawn(move || {
                let outcome = task
                    .run_with(Budget::instructions(1), &mut NoEffects)
                    .unwrap();
                (task, outcome)
            })
            .join()
            .unwrap();
            task = next;
            match outcome {
                RunOutcome::Yielded => {
                    if waits == 1 {
                        conversion_yields += 1;
                    }
                    rt.collect().unwrap();
                }
                RunOutcome::Waiting(_) => {
                    waits += 1;
                    assert_eq!(waits, 1);
                    rt.collect().unwrap();
                    let request = task.take_producer_requests().pop().unwrap();
                    let bigint = clj_quickjs::JsBigInt::from_digits(&digits, 10).unwrap();
                    let value = rt.root(Primitive::BigInt(bigint));
                    task = rt
                        .complete(request.key(), request.producer(), Completion::Ready(&value))
                        .unwrap()
                        .into_runnable()
                        .pop()
                        .unwrap();
                }
                RunOutcome::Complete(value) => {
                    assert_eq!(value.view(), ValueView::Bool(true), "{argument}");
                    assert_eq!(waits, 1);
                    assert!(conversion_yields > 100, "{conversion_yields}");
                    completed = true;
                    break;
                }
                other => panic!("{argument}: {other:?}"),
            }
        }
        assert!(completed, "{argument}");
    }
}

/// Synthetic fixtures, not light-cljs output. Resolve the complete requested
/// list before consulting the host, then use en-US if its default is unsupported.
#[test]
fn intl_number_format_locale_resolution() {
    struct LocaleHost(&'static str, usize);
    impl Host for LocaleHost {
        fn effect(&mut self, _: HostCall) -> Result<Primitive, String> {
            unreachable!()
        }
        fn default_locale(&mut self) -> String {
            self.1 += 1;
            self.0.into()
        }
    }
    for (locales, default, locale, expected, reads) in [
        ("['xx','de']", "fr", "de", "1.234,5", 0),
        ("['xx']", "fr", "fr", "1\u{202f}234,5", 1),
        ("['xx']", "zz", "en-US", "1,234.5", 1),
        ("['de','fr']", "en-US", "de", "1.234,5", 0),
        ("['xx','en-US']", "fr", "en-US", "1,234.5", 0),
    ] {
        let rt = Runtime::new();
        let source = format!(
            "let n=new Intl.NumberFormat({locales}); n.resolvedOptions().locale === '{locale}' && n.format(1234.5) === '{expected}'"
        );
        let task = rt.spawn(&exe(&source), &[]).unwrap();
        let mut host = LocaleHost(default, 0);
        assert!(
            matches!(task.run_with(Budget::instructions(10000),&mut host).unwrap(),RunOutcome::Complete(value) if value.view()==ValueView::Bool(true)),
            "{source}"
        );
        assert_eq!(host.1, reads, "{source}");
    }
}

/// Synthetic fixtures, not light-cljs output. The first 19 rows adapt
/// research/quickjs/tests/test_bigint.js::test_bigint_asintn at 7d1c13c5d8f8.
/// Copyright (c) 2017-2021 Fabrice Bellard and Charlie Gordon.
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
fn bigint_as_int_n_and_as_uint_n() {
    for src in [
        "BigInt.asIntN(0, -1n) === 0n",
        "BigInt.asIntN(1, -1n) === -1n",
        "BigInt.asIntN(8, 128n) === -128n",
        "BigInt.asIntN(8, 127n) === 127n",
        "BigInt.asIntN(64, -1n) === -1n",
        "BigInt.asIntN(64, 0x8000000000000000n) === -0x8000000000000000n",
        "BigInt.asIntN(128, -1n) === -1n",
        "BigInt.asUintN(0, -1n) === 0n",
        "BigInt.asUintN(1, -1n) === 1n",
        "BigInt.asUintN(8, -1n) === 0xffn",
        "BigInt.asUintN(64, -1n) === 0xffffffffffffffffn",
        "BigInt.asUintN(64, -2n) === 0xfffffffffffffffen",
        "BigInt.asUintN(64, 0x8000000000000000n) === 0x8000000000000000n",
        "BigInt.asUintN(128, -1n) === (1n << 128n) - 1n",
        "BigInt.asUintN(192, -1n) === (1n << 192n) - 1n",
        "BigInt.asUintN(64, -(1n << 64n)) === 0n",
        "BigInt.asUintN(64, 1n << 64n) === 0n",
        "BigInt.asUintN(64, 123n) === 123n",
        "BigInt.asIntN(64, 123n) === 123n",
        "BigInt.asUintN(65, -1n) === (1n << 65n) - 1n",
        "BigInt.asIntN(65, 1n << 64n) === -(1n << 64n)",
        "BigInt.asIntN(3, 25n) === 1n",
        "BigInt.asIntN(64, '-1') === -1n",
        "BigInt.asUintN(2**53 - 1, 5n) === 5n",
        "BigInt.asIntN(2**53 - 1, -5n) === -5n",
        "BigInt.asUintN(2**53 - 1, 0n) === 0n",
        "BigInt.asIntN(undefined, -1n) === 0n && BigInt.asUintN(NaN, true) === 0n",
        "BigInt.asUintN('8.9', true) === 1n && BigInt.asIntN(-0.9, false) === 0n",
        "BigInt.asUintN(8, ' 0xff ') === 255n && BigInt.asIntN(8, true) === 1n",
        "let log = ''; let bits = {valueOf() { log += 'b'; return '8'; }}; let n = {valueOf() { log += 'n'; return {}; }, toString() { log += 's'; return '-1'; }}; BigInt.asUintN(bits, n) === 255n && log === 'bns'",
        "let calls = 0; let n = {valueOf() { calls++; return -1n; }}; BigInt.asIntN(0, n) === 0n && calls === 1",
        "let calls = 0; let caught = false; try { BigInt.asIntN(-1, {valueOf() { calls++; return 1n; }}); } catch(e) { caught = e instanceof RangeError; } caught && calls === 0",
        "BigInt.asIntN.call(null, 8, 255n) === -1n && BigInt.asUintN.apply(null, [8, -1n]) === 255n && BigInt.asIntN.bind(null, 8)(128n) === -128n",
        "let a = Object.getOwnPropertyDescriptor(BigInt, 'asIntN'); let b = Object.getOwnPropertyDescriptor(BigInt, 'asUintN'); !a.enumerable && a.writable && a.configurable && !b.enumerable && b.writable && b.configurable && a.value.length === 2 && b.value.length === 2 && a.value.name === 'asIntN' && b.value.name === 'asUintN'",
        "let n = BigInt.asUintN(1048576, -1n); n >> 1048575n === 1n && BigInt.asIntN(1048576, n) === -1n",
        "let n = -(1n << 1048575n); BigInt.asIntN(1048576, n) === n && BigInt.asIntN(1048577, n) === n && BigInt.asUintN(1048576, n) === -n",
        "let n = (1n << 1048575n); BigInt.asUintN(1048577, n) === n && BigInt.asIntN(2**53 - 1, n) === n",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
    for (kind, expressions) in [
        (
            "RangeError",
            &[
                "BigInt.asIntN(-1, 1n)",
                "BigInt.asUintN(-1, 1n)",
                "BigInt.asIntN(2**53, 1n)",
                "BigInt.asUintN(Infinity, 1n)",
                "BigInt.asIntN(-Infinity, 1n)",
                "BigInt.asUintN(2**53 - 1, -1n)",
                "BigInt.asUintN(1048577, -1n)",
            ][..],
        ),
        (
            "TypeError",
            &[
                "BigInt.asIntN(8, 1)",
                "BigInt.asUintN(8, 1)",
                "BigInt.asIntN(0, 1)",
                "BigInt.asIntN(8, null)",
                "BigInt.asUintN(8, undefined)",
                "BigInt.asIntN(8, Symbol())",
                "BigInt.asIntN(1n, 1n)",
                "BigInt.asUintN(Symbol(), 1n)",
                "BigInt.asIntN(8, {valueOf() { return 1; }})",
                "BigInt.asUintN()",
                "new BigInt.asIntN(8, 1n)",
                "new BigInt.asUintN(8, 1n)",
            ][..],
        ),
        (
            "SyntaxError",
            &["BigInt.asIntN(0, '1n')", "BigInt.asUintN(8, '1.5')"][..],
        ),
    ] {
        for expression in expressions {
            assert_eq!(
                eval(&format!(
                    "let caught = false; try {{ {expression}; }} catch(e) {{ caught = e instanceof {kind}; }} caught"
                )),
                ValueView::Bool(true),
                "{expression}"
            );
        }
    }
}

/// Synthetic fixture, not light-cljs output.
#[test]
fn intl_plural_rules_select() {
    for source in [
        "new Intl.PluralRules('en').select(1) === 'one'",
        "new Intl.PluralRules('en').select(2) === 'other'",
        "new Intl.PluralRules('ja').select(1) === 'other'",
        "new Intl.PluralRules('ru').select(1) === 'one'",
        "new Intl.PluralRules('ru').select(2) === 'few'",
        "new Intl.PluralRules('ru').select(5) === 'many'",
        "new Intl.PluralRules('en', {type:'ordinal'}).select(2) === 'two'",
        "new Intl.PluralRules('en-GB').resolvedOptions().locale === 'en-GB'",
        "Intl.PluralRules.supportedLocalesOf(['en','xx']).join() === 'en'",
        "let caught=false; try {new Intl.PluralRules('en',{type:'bogus'});} catch(e){caught=e instanceof RangeError;} caught",
        "let caught=false; try {new Intl.PluralRules('en',5);} catch(e){caught=e instanceof TypeError;} caught",
    ] {
        assert_eq!(eval(source), ValueView::Bool(true), "{source}");
    }
}

/// Synthetic fixture, not light-cljs output.
#[test]
fn intl_plural_rules_options_and_brands() {
    for source in [
        "let r=new Intl.PluralRules('ru'); r.select(1.5) === 'other' && r.select(21) === 'one' && r.select(-2) === 'few' && r.select(Infinity) === 'other' && r.select(NaN) === 'other'",
        "new Intl.PluralRules('en',{minimumFractionDigits:1}).select(1) === 'other'",
        "new Intl.PluralRules('ru',{maximumFractionDigits:0}).select(1.49) === 'one'",
        "new Intl.PluralRules('en',{maximumSignificantDigits:1}).select(1.1) === 'one'",
        "new Intl.PluralRules('en',{minimumSignificantDigits:2}).select(1) === 'other'",
        "let p=new Intl.PluralRules('en',{type:'ordinal'}); p.select(1) === 'one' && p.select(2) === 'two' && p.select(3) === 'few' && p.select(11) === 'other' && p.select(12) === 'other' && p.select(23) === 'few' && p.select(1.2) === 'other'",
        "let p=new Intl.PluralRules('ar'); p.select(0) === 'zero' && p.select(2) === 'two' && p.select(5) === 'few' && p.select(15) === 'many' && p.select(100) === 'other' && p.select(3.5) === 'other'",
        "new Intl.PluralRules('fr').select(1.5) === 'one' && new Intl.PluralRules('fr').select(1000000) === 'many' && new Intl.PluralRules('fr',{type:'ordinal'}).select(1) === 'one'",
        "new Intl.PluralRules('es').select(1) === 'one' && new Intl.PluralRules('es').select(1000000) === 'many' && new Intl.PluralRules('es',{type:'ordinal'}).select(1) === 'other'",
        "new Intl.PluralRules('pt-BR').select(0) === 'one' && new Intl.PluralRules('tr').select(1) === 'one' && new Intl.PluralRules('de').select(1) === 'one' && new Intl.PluralRules('zh-Hant').select(1) === 'other'",
        "new Intl.PluralRules('en-US-u-nu-arab').resolvedOptions().locale === 'en-US' && new Intl.PluralRules(['xx','de-DE']).resolvedOptions().locale === 'de'",
        "Intl.PluralRules.supportedLocalesOf(['EN-us-u-nu-arab','de-DE','xx']).join() === 'en-US-u-nu-arab,de-DE'",
        "let p=new Intl.PluralRules('ru'); let a=p.resolvedOptions(); a.pluralCategories.join() === 'one,few,many,other' && a.minimumIntegerDigits === 1 && a.minimumFractionDigits === 0 && a.maximumFractionDigits === 3 && a.type === 'cardinal' && Object.keys(a).join() === 'locale,type,minimumIntegerDigits,minimumFractionDigits,maximumFractionDigits,pluralCategories,roundingMode'",
        "let p=new Intl.PluralRules('en'); p instanceof Intl.PluralRules && p[Symbol.toStringTag] === 'Intl.PluralRules' && Intl.PluralRules.name === 'PluralRules' && Intl.PluralRules.length === 1 && p.select.length === 1 && p.resolvedOptions.length === 0 && Intl.PluralRules.supportedLocalesOf.length === 1",
        "let p=new Intl.PluralRules('en'); let a=p.resolvedOptions(); a.pluralCategories.push('bogus'); p.resolvedOptions().pluralCategories.join() === 'one,other'",
    ] {
        assert_eq!(eval(source), ValueView::Bool(true), "{source}");
    }
    for call in [
        "Intl.PluralRules()",
        "new Intl.PluralRules('en',null)",
        "Intl.PluralRules.prototype.select.call({},1)",
        "Intl.PluralRules.prototype.resolvedOptions.call({})",
        "new Intl.PluralRules('en').select(1n)",
        "new Intl.PluralRules('en').select(Symbol())",
        "new Intl.PluralRules.supportedLocalesOf('en')",
        "Intl.PluralRules.supportedLocalesOf('en',5)",
    ] {
        let source = format!(
            "let caught=false; try {{{call};}} catch(e) {{caught=e instanceof TypeError;}} caught"
        );
        assert_eq!(eval(&source), ValueView::Bool(true), "{source}");
    }
    for options in [
        "{minimumIntegerDigits:0}",
        "{minimumFractionDigits:101}",
        "{maximumFractionDigits:NaN}",
        "{minimumFractionDigits:3,maximumFractionDigits:2}",
        "{minimumSignificantDigits:4,maximumSignificantDigits:3}",
        "{localeMatcher:'bogus'}",
    ] {
        let source = format!(
            "let caught=false; try {{new Intl.PluralRules('en',{options});}} catch(e) {{caught=e instanceof RangeError;}} caught"
        );
        assert_eq!(eval(&source), ValueView::Bool(true), "{source}");
    }
}

/// Synthetic fixture, not light-cljs output.
#[test]
fn intl_plural_rules_callbacks_wait_collect_and_migrate() {
    let rt = Runtime::new();
    let source = r#"
        let log='';
        let n=new Intl.PluralRules({get length(){log+='l';__rt.need(1,1);return 1;},get 0(){log+='t';__rt.need(2,1);return 'RU';}},
            {get maximumFractionDigits(){log+='f';__rt.need(3,1);return {valueOf(){log+='v';__rt.need(4,1);return 1;}};}});
        let f=n.select.bind(n); n=null;
        let text=f({valueOf(){log+='n';__rt.need(6,1);return 2.25;}});
        let big=f({valueOf(){log+='b';__rt.need(7,1);return 21;}});
        text === 'other' && big === 'one' && log === 'ltfvnb'
    "#;
    let mut task = rt.spawn(&exe(source), &[]).unwrap();
    let mut waited = 0;
    for _ in 0..2000 {
        let (next, outcome) = std::thread::spawn(move || {
            let outcome = task
                .run_with(Budget::instructions(1), &mut NoEffects)
                .unwrap();
            (task, outcome)
        })
        .join()
        .unwrap();
        task = next;
        match outcome {
            RunOutcome::Yielded => {
                rt.collect().unwrap();
            }
            RunOutcome::Waiting(_) => {
                waited += 1;
                assert!(waited <= 6);
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
            RunOutcome::Complete(value) => {
                assert_eq!(value.view(), ValueView::Bool(true));
                assert_eq!(waited, 6);
                return;
            }
            other => panic!("{other:?}"),
        }
    }
    panic!("PluralRules continuation did not finish");
}

/// Synthetic fixture, not light-cljs output.
#[test]
fn intl_plural_rules_ownership_and_abrupt_options() {
    for source in [
        "let log=''; let caught=false; try {new Intl.PluralRules('en',{get type(){log+='t';throw 9;},get minimumIntegerDigits(){log+='i';return 1;}});} catch(e){caught=e === 9;} caught && log === 't'",
        "let caught=false; let later=false; try {new Intl.PluralRules('en',{minimumSignificantDigits:4,maximumSignificantDigits:3,get roundingMode(){later=true;throw 9;}});} catch(e){caught=e instanceof RangeError;} caught && !later",
        "let caught=false; try {new Intl.PluralRules('en',{maximumFractionDigits:1n});} catch(e){caught=e instanceof TypeError;} caught",
        "let log=''; let caught=false; try {Intl.PluralRules.prototype.select.call({}, {valueOf(){log+='n';return 1;}});} catch(e){caught=e instanceof TypeError;} caught && log === ''",
        "let caught=false; let log=''; try {new Intl.PluralRules('en').select({valueOf(){log+='n';throw 9;}});} catch(e){caught=e === 9;} caught && log === 'n'",
    ] {
        assert_eq!(eval(source), ValueView::Bool(true), "{source}");
    }
    let rt = Runtime::new();
    for source in [
        "new Intl.PluralRules('en')",
        "Object.freeze(new Intl.PluralRules('en'))",
    ] {
        let owner = rt.spawn(&exe(source), &[]).unwrap();
        let RunOutcome::Complete(value) = owner.run().unwrap() else {
            panic!("expected PluralRules");
        };
        assert!(matches!(
            rt.publish(&value),
            Err(clj_quickjs::PublishError::NotShareable { .. })
        ));
        for body in [
            "Intl.PluralRules.prototype.select.call(value,1)",
            "Intl.PluralRules.prototype.resolvedOptions.call(value)",
        ] {
            let code = compile_function(&format!("function f(value){{return {body};}}")).unwrap();
            let caller = rt.spawn(&code, &[&value]).unwrap();
            assert!(
                matches!(caller.run().unwrap(),RunOutcome::Failed(error) if matches!(error.kind(),VmErrorKind::ForeignObject{..}))
            );
        }
    }
    rt.collect().unwrap();
}

/// Synthetic fixture, not light-cljs output. The six operands distinguish
/// directed rounding, sign-sensitive ties and halfEven's even neighbors.
#[test]
fn intl_plural_rules_rounding_modes() {
    for (mode, expected) in [
        ("ceil", ["other", "one", "one", "other", "other", "one"]),
        ("floor", ["one", "other", "other", "one", "one", "other"]),
        ("expand", ["other", "other", "one", "one", "other", "other"]),
        ("trunc", ["one", "one", "other", "other", "one", "one"]),
        ("halfCeil", ["one", "one", "one", "other", "other", "one"]),
        ("halfFloor", ["one", "one", "other", "one", "one", "other"]),
        ("halfExpand", ["one", "one", "one", "one", "other", "other"]),
        ("halfTrunc", ["one", "one", "other", "other", "one", "one"]),
        (
            "halfEven",
            ["one", "one", "other", "other", "other", "other"],
        ),
    ] {
        let expected = expected.join(",");
        let source = format!(
            "let p=new Intl.PluralRules('en',{{maximumFractionDigits:0,roundingMode:'{mode}'}}); [p.select(1.1),p.select(-1.1),p.select(0.5),p.select(-0.5),p.select(1.5),p.select(-1.5)].join() === '{expected}' && p.resolvedOptions().roundingMode === '{mode}'"
        );
        assert_eq!(eval(&source), ValueView::Bool(true), "{source}");
    }
    assert_eq!(
        eval(
            "let caught=false; try {new Intl.PluralRules('en',{roundingMode:'bogus'});} catch(e){caught=e instanceof RangeError;} caught"
        ),
        ValueView::Bool(true)
    );
}

/// Synthetic fixtures, not light-cljs output. Locale rows adapt QuickJS's
/// tests/test_intl.js::test_numberFormat (7d1c13c5d8f8).
/// MIT license: see the full QuickJS copyright and permission
/// notice above intl_number_format_decimal and src/intl/data/QUICKJS-LICENSE.
#[test]
fn intl_number_format_percent_and_currency() {
    // Policy clj-quickjs-209 selects QuickJS's four-digit Spanish grouping.
    for (locale, style, number, expected) in [
        ("en", "percent", "0.5", "50%"),
        ("en", "currency", "1234.56", "€1,234.56"),
        ("de", "percent", "0.5", "50\u{a0}%"),
        ("de", "currency", "1234.56", "1.234,56\u{a0}€"),
        ("fr", "percent", "0.5", "50\u{a0}%"),
        ("fr", "currency", "1234.56", "1\u{202f}234,56\u{a0}€"),
        ("es", "currency", "1234.56", "1.234,56\u{a0}€"),
        ("ar", "percent", "0.5", "50\u{200e}%\u{200e}"),
        ("ar", "currency", "1234.56", "\u{200f}1,234.56\u{a0}€"),
        ("ru", "currency", "1234.56", "1\u{a0}234,56\u{a0}€"),
        ("pt-BR", "currency", "1234.56", "€\u{a0}1.234,56"),
        ("ja", "currency", "1234.5", "€1,234.50"),
    ] {
        let source = format!(
            "new Intl.NumberFormat('{locale}',{{style:'{style}',currency:'EUR'}}).format({number}) === '{expected}'"
        );
        assert_eq!(eval(&source), ValueView::Bool(true), "{source}");
    }
    for source in [
        "new Intl.NumberFormat('ja',{style:'currency',currency:'JPY'}).format(1234.5) === '￥1,235'",
        "let o=new Intl.NumberFormat('en',{style:'currency',currency:'eur'}).resolvedOptions(); o.style === 'currency' && o.currency === 'EUR' && o.currencyDisplay === 'symbol' && o.currencySign === 'standard' && o.minimumFractionDigits === 2 && o.maximumFractionDigits === 2",
        "let caught=false; try {new Intl.NumberFormat('en',{style:'currency'});} catch(e){caught=e instanceof TypeError;} caught",
        "let caught=false; try {new Intl.NumberFormat('en',{style:'currency',currency:'EU'});} catch(e){caught=e instanceof RangeError;} caught",
    ] {
        assert_eq!(eval(source), ValueView::Bool(true), "{source}");
    }
}

/// Synthetic fixtures, not light-cljs output. Snapshot compatibility includes
/// symbol/name aliases, nonfinite values without affixes, and CLF=3 digits.
#[test]
fn intl_number_format_currency_options_and_percent_scaling() {
    for source in [
        "let f=new Intl.NumberFormat('en',{style:'percent'}).format; f('0.005') === '1%' && f('0.0049') === '0%' && f(-0) === '-0%' && f(0) === '0%' && f(12n) === '1,200%' && f('12345678901234567890.12') === '1,234,567,890,123,456,789,012%'",
        "new Intl.NumberFormat('en',{style:'percent',maximumFractionDigits:2}).format('0.012345') === '1.23%'",
        "new Intl.NumberFormat('en',{style:'percent',minimumSignificantDigits:3,maximumSignificantDigits:3}).format('0.012345') === '1.23%'",
        "let n=new Intl.NumberFormat('en',{style:'percent'}); let o=n.resolvedOptions(); o.style === 'percent' && o.minimumFractionDigits === 0 && o.maximumFractionDigits === 0 && o.currency === undefined && o.currencyDisplay === undefined && o.currencySign === undefined",
        "let n=new Intl.NumberFormat('en',{style:'currency',currency:'USD',currencySign:'accounting'}); n.format(-1234.5) === '($1,234.50)' && n.format(-0) === '($0.00)' && n.format(0) === '$0.00' && n.resolvedOptions().currencySign === 'accounting'",
        "new Intl.NumberFormat('de',{style:'currency',currency:'EUR',currencySign:'accounting'}).format(-1) === '-1,00\u{00a0}€'",
        "new Intl.NumberFormat('ar',{style:'currency',currency:'EUR'}).format(-1) === '\u{200f}-1.00\u{00a0}€'",
        "new Intl.NumberFormat('ar',{style:'currency',currency:'EUR',currencySign:'accounting'}).format(-1) === '(\u{061c}1.00€)'",
        "let f=new Intl.NumberFormat('ar',{style:'currency',currency:'EUR',currencySign:'accounting'}).format; f(1) === '\u{061c}1.00€' && f(0) === '\u{061c}0.00€'",
        "new Intl.NumberFormat('ar',{style:'percent'}).format(-0.5) === '\u{200e}-50\u{200e}%\u{200e}'",
        "new Intl.NumberFormat('tr',{style:'percent'}).format(-0.5) === '-%50'",
        "let n=new Intl.NumberFormat('en',{style:'currency',currency:'EUR',currencyDisplay:'code'}); n.format(1) === 'EUR1.00' && n.resolvedOptions().currencyDisplay === 'code'",
        "let n=new Intl.NumberFormat('en',{style:'currency',currency:'CAD',currencyDisplay:'narrowSymbol'}); n.format(1) === 'CA$1.00' && n.resolvedOptions().currencyDisplay === 'narrowSymbol'",
        "let n=new Intl.NumberFormat('en',{style:'currency',currency:'EUR',currencyDisplay:'name'}); n.format(1) === '€1.00' && n.resolvedOptions().currencyDisplay === 'name'",
        "new Intl.NumberFormat('en',{style:'currency',currency:'xyz'}).format(1) === 'XYZ1.00'",
        "new Intl.NumberFormat('en',{style:'currency',currency:'KWD'}).format(1.2345) === 'KWD1.235'",
        "new Intl.NumberFormat('en',{style:'currency',currency:'CLF'}).resolvedOptions().maximumFractionDigits === 3",
        "new Intl.NumberFormat('en',{style:'currency',currency:'EUR',maximumFractionDigits:0}).format(1.5) === '€2'",
        "new Intl.NumberFormat('en',{style:'currency',currency:'JPY',minimumFractionDigits:2}).format(1.5) === '¥1.50'",
        "new Intl.NumberFormat('en',{style:'currency',currency:'EUR',minimumSignificantDigits:3,maximumSignificantDigits:3}).format(1234.5) === '€1,230'",
        "new Intl.NumberFormat('en',{style:'currency',currency:'EUR',numberingSystem:'fullwide'}).format(1234.5) === '€１,２３４.５０'",
        "let f=new Intl.NumberFormat('en',{style:'currency',currency:'EUR',currencySign:'accounting'}).format; f(NaN) === 'NaN' && f(Infinity) === '∞' && f(-Infinity) === '-∞'",
        "let f=new Intl.NumberFormat('en',{style:'percent'}).format; f(NaN) === 'NaN' && f(Infinity) === '∞'",
        "let o=new Intl.NumberFormat('en',{style:'decimal',currency:'eur',currencyDisplay:'code',currencySign:'accounting'}).resolvedOptions(); o.currency === undefined && o.currencyDisplay === undefined && o.currencySign === undefined",
        "let log=''; let caught=false; try {new Intl.NumberFormat('en',{style:'currency',get currencyDisplay(){log+='d';return 'symbol';},get currencySign(){log+='s';return 'standard';},get notation(){log+='n';return 'standard';}});} catch(e){caught=e instanceof TypeError;} caught && log === 'ds'",
        "let log=''; let caught=false; try {new Intl.NumberFormat('en',{currency:'EU',get currencyDisplay(){log+='d';return 'symbol';}});} catch(e){caught=e instanceof RangeError;} caught && log === ''",
        "let caught=false; try {new Intl.NumberFormat('en',{style:'unit'});} catch(e){caught=e instanceof RangeError;} caught",
    ] {
        assert_eq!(eval(source), ValueView::Bool(true), "{source}");
    }
    for option in [
        "currency:'EU'",
        "currency:'E1R'",
        "currency:'€UR'",
        "currency:'EUR\\0'",
        "currencyDisplay:'unknown'",
        "currencySign:'unknown'",
    ] {
        let source = format!("new Intl.NumberFormat('en',{{{option}}})");
        assert_eq!(failure(&source).kind(), VmErrorKind::RangeError, "{source}");
    }
}

/// Synthetic fixture, not light-cljs output. New string options and their
/// conversions suspend independently, keeping exact getter order across GC.
#[test]
fn intl_number_format_currency_callbacks_wait_collect_and_migrate() {
    let rt = Runtime::new();
    let source = r#"
        let log='';
        let n=new Intl.NumberFormat('en',{
            get style(){log+='s';__rt.need(1,1);return {toString(){log+='S';__rt.need(2,1);return 'currency';}};},
            get currency(){log+='c';__rt.need(3,1);return {toString(){log+='C';__rt.need(4,1);return 'eur';}};},
            get currencyDisplay(){log+='d';__rt.need(5,1);return {toString(){log+='D';__rt.need(6,1);return 'code';}};},
            get currencySign(){log+='a';__rt.need(7,1);return {toString(){log+='A';__rt.need(8,1);return 'accounting';}};}
        });
        let f=n.format; n=null;
        let text=f({valueOf(){log+='n';__rt.need(9,1);return -12n;}});
        text === '(EUR12.00)' && log === 'sScCdDaAn'
    "#;
    let mut task = rt.spawn(&exe(source), &[]).unwrap();
    let mut waited = 0;
    for _ in 0..3000 {
        let (next, outcome) = std::thread::spawn(move || {
            let outcome = task
                .run_with(Budget::instructions(1), &mut NoEffects)
                .unwrap();
            (task, outcome)
        })
        .join()
        .unwrap();
        task = next;
        match outcome {
            RunOutcome::Yielded => {
                rt.collect().unwrap();
            }
            RunOutcome::Waiting(_) => {
                waited += 1;
                assert!(waited <= 9);
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
            RunOutcome::Complete(value) => {
                assert_eq!(value.view(), ValueView::Bool(true));
                assert_eq!(waited, 9);
                return;
            }
            other => panic!("{other:?}"),
        }
    }
    panic!("currency option continuation did not finish");
}

// ISO rows adapted from QuickJS tests/test_builtin.js::test_date,
// revision 7d1c13c5d8f8. Synthetic fixtures, not light-cljs output.
// Copyright (c) 2017-2021 Fabrice Bellard
// Copyright (c) 2017-2021 Charlie Gordon
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL
// THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
// THE SOFTWARE.
#[test]
fn date_parse_iso_strings() {
    use clj_quickjs::host::{TimeZone, sim::SimTimeHost};

    for offset in [0, 3600] {
        let rt = Runtime::new();
        let mut host = SimTimeHost::new(0.0, TimeZone::fixed_offset_seconds(offset).unwrap());
        let mut sources = Vec::new();
        for (text, expected) in [
            ("0000", -62167219200000_i64),
            ("0001", -62135596800000),
            ("0099", -59042995200000),
            ("+000000-01-01T00:00Z", -62167219200000),
            ("2000", 946684800000_i64),
            ("2000-01", 946684800000),
            ("2000-01-01", 946684800000),
            ("2000-01-01T00:00Z", 946684800000),
            ("2000-01-01T00:00:00Z", 946684800000),
            ("2000-01-01T00:00:00+00:00", 946684800000),
            ("2000-01-01T00:00:00.1Z", 946684800100),
            ("2000-01-01T00:00:00.10Z", 946684800100),
            ("2000-01-01T00:00:00.100Z", 946684800100),
            ("2000-01-01T00:00:00.1000Z", 946684800100),
            ("2000-01-01T00:00:00.123456789Z", 946684800123),
            ("+002000-01-01T00:00:00Z", 946684800000),
            ("-000001-01-01T00:00:00Z", -62198755200000),
            ("2000-01-01T24:00Z", 946771200000),
            ("2000-01-01T00:00+00:30", 946683000000),
            ("2000-01-01T00:00-01:30", 946690200000),
            ("+275760-09-13T00:00:00.000Z", 8640000000000000),
            ("-271821-04-20T00:00:00.000Z", -8640000000000000),
        ] {
            sources.push(format!(
                "Date.parse('{text}') === {expected} && new Date('{text}').getTime() === {expected}"
            ));
        }
        for text in [
            "",
            "-000000-01-01T00:00:00Z",
            "2000-13",
            "2000-00",
            "2000-01-00",
            "2000-01-32",
            "2000-01-01T25:00Z",
            "2000-01-01T24:00:01Z",
            "2000-01-01T24:00:00.001Z",
            "2000-01-01T00:60Z",
            "2000-01-01T00:00:60Z",
            "2000-01-01T00:00+24:00",
            "2000-01-01T00:00+00:60",
            "2000-01-01T00:00:00.Z",
            "2000-01-01T00:00:00.123Zjunk",
            "2000-01-01T00:00:00.123Z\u{0}",
            "2000-01-01T00:00:00.123Z\u{d7ff}",
            "+275760-09-13T00:00:00.001Z",
            "-271821-04-19T23:59:59.999Z",
        ] {
            // Debug quoting escapes the NUL and preserves the non-ASCII unit.
            sources.push(format!(
                "Number.isNaN(Date.parse({text:?})) && Number.isNaN(new Date({text:?}).getTime())"
            ));
        }
        for (fraction, expected) in [
            ("123", "123"),
            ("1", "100"),
            ("12", "120"),
            ("1234", "123"),
            ("12345", "123"),
            ("1235", "123"),
            ("9999", "999"),
        ] {
            sources.push(format!("new Date('2020-01-01T01:01:01.{fraction}Z').toISOString() === '2020-01-01T01:01:01.{expected}Z'"));
        }
        let local = 946684800000_i64 - i64::from(offset) * 1000;
        let local_iso = if offset == 0 {
            "2000-01-01T00:00:00.000Z"
        } else {
            "1999-12-31T23:00:00.000Z"
        };
        sources.push(format!("new Date('2000T00:00').toISOString() === '{local_iso}' && new Date('2000-01-01T00:00').getTime() === {local} && Date.parse('2000-01T00:00') === {local} && Date.parse('2000-01-01') === 946684800000"));
        sources.push("let d = new Date(1506098258091); d.setUTCHours(18, 10, 11); new Date(Date.parse(d.toISOString())).toISOString() === d.toISOString()".into());
        sources.push("let p = Object.getOwnPropertyDescriptor(Date, 'parse'); p.value === Date.parse && p.writable && !p.enumerable && p.configurable && Date.parse.name === 'parse' && Date.parse.length === 1 && Number.isNaN(Date.parse())".into());
        for source in sources {
            let task = rt
                .spawn(&compile_script("date-parse.js", &source).unwrap(), &[])
                .unwrap();
            let mut completed = false;
            for _ in 0..10000 {
                match task.run_with(Budget::instructions(1), &mut host).unwrap() {
                    RunOutcome::Yielded => {
                        rt.collect().unwrap();
                    }
                    RunOutcome::Complete(value) => {
                        assert_eq!(
                            value.view(),
                            ValueView::Bool(true),
                            "offset {offset}: {source}"
                        );
                        completed = true;
                        break;
                    }
                    other => panic!("{source}: {other:?}"),
                }
            }
            assert!(completed, "bounded ISO parse completes: {source}");
        }
        assert_eq!(host.clock_reads(), 0);
    }
}

/// Synthetic fixtures, not light-cljs output. String conversion uses the
/// string hint for parse and the default hint for construction, exactly once.
#[test]
fn date_parse_coercion_and_long_fractions() {
    for source in [
        "Date.parse(2000) === 946684800000 && Date.parse(2000n) === 946684800000 && Date.parse({toString() {return 2000n;}}) === 946684800000",
        "let log = ''; Date.parse({toString() {log += 's'; return {};}, valueOf() {log += 'v'; return '2000';}}) === 946684800000 && log === 'sv'",
        "let log = ''; new Date({valueOf() {log += 'v'; return {};}, toString() {log += 's'; return '2000';}}).getTime() === 946684800000 && log === 'vs'",
        "Date.parse('2000', {toString() {throw 1;}}) === 946684800000 && Number.isNaN(Date.parse(undefined)) && Number.isNaN(Date.parse(null))",
        "let caught = false; try {Date.parse(Symbol());} catch(e) {caught = e instanceof TypeError;} caught",
        "let caught = false; try {new Date.parse('2000');} catch(e) {caught = e instanceof TypeError;} caught",
        "let caught = false; try {Date.parse({toString() {throw 42;}});} catch(e) {caught = e === 42;} caught",
        "let text = '2000-01-01T00:00:00.123' + ''.padEnd(10000, '9') + 'Z'; Date.parse(text) === 946684800123 && new Date(text).getTime() === 946684800123 && Number.isNaN(Date.parse(text + 'x')) && Number.isNaN(Date.parse(text + '\\ud800'))",
        "Date.parse('2000-02-31') === Date.UTC(2000, 2, 2) && Date.parse('+275760-09-13T01:00+01:00') === 8.64e15 && Date.parse('-271821-04-19T23:00-01:00') === -8.64e15",
    ] {
        assert_eq!(eval(source), ValueView::Bool(true), "{source}");
    }
}

/// Synthetic fixture, not light-cljs output. Coercion getters and methods
/// survive waits, collection, and worker migration before entering the parser.
#[test]
fn date_parse_waits_collects_and_migrates() {
    let rt = Runtime::new();
    let source = r"
        let log = '';
        let parsed = Date.parse({get toString() {
            log += 'g'; __rt.need(1, 1);
            return function() {log += 's'; __rt.need(2, 1); return '2000';};
        }});
        let d = new Date({get valueOf() {
            log += 'v'; __rt.need(3, 1);
            return function() {log += 'c'; __rt.need(4, 1); return '2000-01-01T00:00:00.123Z';};
        }});
        parsed === 946684800000 && d.getTime() === 946684800123 && log === 'gsvc'
    ";
    let mut task = rt.spawn(&exe(source), &[]).unwrap();
    let mut waited = 0;
    let mut completed = false;
    for _ in 0..2000 {
        let (next, outcome) = std::thread::spawn(move || {
            let outcome = task
                .run_with(Budget::instructions(1), &mut NoEffects)
                .unwrap();
            (task, outcome)
        })
        .join()
        .unwrap();
        task = next;
        match outcome {
            RunOutcome::Yielded => {
                rt.collect().unwrap();
            }
            RunOutcome::Waiting(_) => {
                waited += 1;
                assert!(waited <= 4);
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
            RunOutcome::Complete(value) => {
                assert_eq!(value.view(), ValueView::Bool(true));
                completed = true;
                break;
            }
            other => panic!("{other:?}"),
        }
    }
    assert!(completed);
    assert_eq!(waited, 4);
}

/// Synthetic fixtures, not light-cljs output.
#[test]
fn array_sort_default_and_comparator() {
    for src in [
        "[3, 1, 10, 2].sort().join() === '1,10,2,3'",
        "let a = [3, 1, 10, 2]; a.sort(function(a,b) { return a-b; }) === a && a.join() === '1,2,3,10'",
        "let a = [{k:1,v:'a'}, {k:0,v:'b'}, {k:1,v:'c'}]; a.sort(function(a,b) { return a.k-b.k; }); a[0].v === 'b' && a[1].v === 'a' && a[2].v === 'c'",
        "let a = [undefined, 2, , 1]; a.sort(); a.join() === '1,2,,' && a.length === 4 && a[2] === undefined && (2 in a) && !(3 in a)",
        "let ok = false; try { [1].sort(5); } catch(e) { ok = e instanceof TypeError; } ok",
        "let reads = 0, o = {get length() { reads++; return 1; }, get 0() { reads++; return 1; }}; let ok = false; try { Array.prototype.sort.call(o, 5); } catch(e) { ok = e instanceof TypeError; } ok && reads === 0",
        "let a = [3,2,1]; a.sort(function(a,b) { return {valueOf() { return a-b; }}; }); a.join() === '1,2,3'",
        "let a = [3,2,1]; a.sort(function() { return NaN; }); a.join() === '3,2,1'",
        "let a = [3,2,1]; a.sort(function() { return '0'; }); a.join() === '3,2,1'",
        "let calls = 0; [undefined, 1, undefined].sort(function() { calls++; return 0; }); calls === 0",
        "let a = [{toString() { return 'b'; }}, {toString() { return 'a'; }}]; a.sort(); a[0].toString() === 'a'",
        "['\\ue000', '\\ud800\\udc00', '\\ud800'].sort().join('|') === '\\ud800|\\ud800\\udc00|\\ue000'",
        "let a = [3,2,1], e = {}; let caught; try { a.sort(function() { throw e; }); } catch(x) { caught = x; } caught === e && a.join() === '3,2,1'",
        "let o = {length: 4, 0: undefined, 2: 2, 3: 1}; Array.prototype.sort.call(o) === o && o[0] === 1 && o[1] === 2 && o[2] === undefined && !(3 in o)",
        "let a = [,2,1]; Object.setPrototypeOf(a, {0: 3}); Array.prototype.sort.call(a); a[0] === 1 && a[1] === 2 && a[2] === 3",
        "let a = [2,1]; Object.defineProperty(a, '0', {writable: false}); let ok = false; try { a.sort(); } catch(e) { ok = e instanceof TypeError; } ok",
        "Array.prototype.sort.length === 1 && Array.prototype.sort.name === 'sort' && !Object.getOwnPropertyDescriptor(Array.prototype, 'sort').enumerable",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
}

/// Synthetic fixture, not light-cljs output. The long common prefix must
/// yield while flattening/comparing, between ToString and the first write.
#[test]
fn array_sort_default_comparison_is_budgeted() {
    #[derive(Default)]
    struct Log(Vec<f64>);
    impl Host for Log {
        fn effect(&mut self, call: HostCall) -> Result<Primitive, String> {
            if let Primitive::Number(n) = call.arg {
                self.0.push(n);
            }
            Ok(Primitive::Undefined)
        }
    }
    let source = "let text = 'x'.padEnd(8192,'x'); let o = {length: 2, get 0() { return {toString() { __rt.effect(1,1); return text + 'b'; }}; }, set 0(v) { __rt.effect(1,3); }, get 1() { return {toString() { __rt.effect(1,2); return text + 'a'; }}; }, set 1(v) { __rt.effect(1,4); }}; Array.prototype.sort.call(o); true";
    let rt = Runtime::new();
    let task = rt.spawn(&exe(source), &[]).unwrap();
    let mut host = Log::default();
    let mut comparison_yields = 0;
    for _ in 0..20000 {
        match task.run_with(Budget::instructions(1), &mut host).unwrap() {
            RunOutcome::Yielded => {
                if host.0.len() == 2 {
                    comparison_yields += 1;
                }
                rt.collect().unwrap();
            }
            RunOutcome::Complete(value) => {
                assert_eq!(value.view(), ValueView::Bool(true));
                assert_eq!(host.0, [1., 2., 3., 4.]);
                assert!(
                    comparison_yields >= 32,
                    "long comparison ran without yielding"
                );
                return;
            }
            other => panic!("unexpected sort outcome: {other:?}"),
        }
    }
    panic!("sort did not complete");
}

/// Synthetic fixtures, not light-cljs output.
#[test]
fn array_sort_merge_boundaries_and_errors() {
    for src in [
        "let ok = true; for (let n = 0; n < 67; n++) { let a = []; for (let i = 0; i < n; i++) a.push({k: (i * 13) % 7, i: i}); a.sort(function(a,b) { return a.k-b.k; }); if (a.length !== n) ok = false; for (let i = 1; i < n; i++) if (a[i-1].k > a[i].k || (a[i-1].k === a[i].k && a[i-1].i > a[i].i)) ok = false; } ok",
        "let a = [2,,1]; Object.defineProperty(a, '2', {configurable: false}); let ok = false; try { a.sort(); } catch(e) { ok = e instanceof TypeError; } ok && a[0] === 1 && a[1] === 2",
        "let e = {}, caught; try { [{toString() { throw e; }}, 1].sort(); } catch(x) { caught = x; } caught === e",
        "let e = {}, caught; try { [2,1].sort(function() { return {valueOf() { throw e; }}; }); } catch(x) { caught = x; } caught === e",
        "let ok = false; try { [2,1].sort(function() { return 1n; }); } catch(e) { ok = e instanceof TypeError; } ok",
        "let ok = false; try { [Symbol(), 1].sort(); } catch(e) { ok = e instanceof TypeError; } ok",
        "let ok = false; [2,1].sort(function(a,b) { 'use strict'; ok = this === undefined; return a-b; }); ok",
        "let n = Array.prototype.sort.call(5); typeof n === 'object' && n.valueOf() === 5",
        "let ok = false; try { Array.prototype.sort.call('ba'); } catch(e) { ok = e instanceof TypeError; } ok",
        "let count = 0; let a = [2,1]; a.sort(function(a,b) { count++; return a-b; }.bind({})); count === 1 && a.join() === '1,2'",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
}

/// Synthetic fixtures, not light-cljs output. Native leaf callbacks and
/// built-ins that push their own conversion continuation deliver results
/// through the same comparator boundary as JavaScript functions.
#[test]
fn array_sort_native_comparators() {
    for src in [
        "let a = [3,1,2]; a.sort(Number.isNaN) === a && a.join() === '3,1,2'",
        "[3,1,2].sort(Number.isNaN.bind(null)).join() === '3,1,2'",
        "[1,2].sort(Number).join() === '2,1'",
        "let a = [{i:3,valueOf() { return 0; }}, {i:1,valueOf() { return 0; }}, {i:2,valueOf() { return 0; }}]; a.sort(Number); a[0].i === 3 && a[1].i === 1 && a[2].i === 2",
    ] {
        assert_eq!(eval(src), ValueView::Bool(true), "{src}");
    }
}

/// Synthetic fixtures, not light-cljs output. Sorting rows follow QuickJS
/// tests/test_intl.js at 7d1c13c5d8f8; its license notice appears above
/// intl_number_format_decimal and in src/intl/data/QUICKJS-LICENSE.
#[test]
fn intl_collator_root_order() {
    for source in [
        "new Intl.Collator('en').compare('a','b') < 0",
        "new Intl.Collator('en',{sensitivity:'base'}).compare('a','A') === 0",
        "new Intl.Collator('en',{sensitivity:'accent'}).compare('a','á') !== 0",
        "new Intl.Collator('en',{numeric:true}).compare('2','10') < 0",
        "let c=new Intl.Collator('en'); let f=c.compare; f === c.compare && f('a','b') < 0 && f.length === 2 && f.name === ''",
        "let c=new Intl.Collator('en'); c.compare('a','A') < 0 && c.compare('A','á') < 0 && c.compare('á','b') < 0 && c.compare('é','é') === 0",
        "new Intl.Collator('en').resolvedOptions().collation === 'default'",
        "let caught=false; try {new Intl.Collator('en',{sensitivity:'bogus'});} catch(e){caught=e instanceof RangeError;} caught",
        // Required upstream rows: intentionally fail while the separately
        // assigned Array.prototype.sort prerequisite (-208) is absent.
        "['z','a','Z','A'].sort(new Intl.Collator('en').compare).join(',') === 'a,A,z,Z'",
        "['z','a','Z','A'].sort(new Intl.Collator('de').compare).join(',') === 'a,A,z,Z'",
    ] {
        assert_eq!(eval(source), ValueView::Bool(true), "{source}");
    }
}

/// Synthetic fixtures, not light-cljs output.
#[test]
fn intl_collator_options_and_brands() {
    for source in [
        "let c=Intl.Collator('en'); c instanceof Intl.Collator && c[Symbol.toStringTag] === 'Intl.Collator' && Intl.Collator.length === 0 && Intl.Collator.supportedLocalesOf.length === 1",
        "let c=new Intl.Collator('en',{sensitivity:'case'}); c.compare('á','a') === 0 && c.compare('a','A') < 0",
        "new Intl.Collator('en',{caseFirst:'upper'}).compare('a','A') > 0 && new Intl.Collator('en',{caseFirst:'lower'}).compare('a','A') < 0",
        "new Intl.Collator('en',{ignorePunctuation:true}).compare('a- b!','ab') === 0",
        "new Intl.Collator('en',{numeric:true}).compare('x0002','x2') === 0 && new Intl.Collator('en').compare('item2','item10') > 0",
        "let c=new Intl.Collator('en',{sensitivity:'base'}); c.compare('Straße','STRASSE') === 0 && c.compare('Σ','σ') === 0",
        r"new Intl.Collator('en').compare('\ud800','\ud801') < 0 && new Intl.Collator('en').compare('\ud800','�') !== 0",
        r"new Intl.Collator('en').compare('a\u0315\u0300','à\u0315') === 0 && new Intl.Collator('en').compare('각','\u1100\u1161\u11a8') === 0",
        "Intl.Collator.supportedLocalesOf(['EN-us-u-kn','de-DE','xx']).join() === 'en-US-u-kn,de-DE'",
        "let c=new Intl.Collator('en-US-u-kf-upper-kn'); c.resolvedOptions().numeric && c.resolvedOptions().caseFirst === 'upper' && c.compare('2','10') < 0 && c.compare('a','A') > 0",
        "new Intl.Collator('en-u-kf-upper-kn',{numeric:false,caseFirst:'lower'}).resolvedOptions().locale === 'en'",
        "new Intl.Collator('en-u-co-phonebk').resolvedOptions().collation === 'default' && new Intl.Collator('de',{usage:'search',collation:'phonebk'}).resolvedOptions().collation === 'default'",
        "let c=new Intl.Collator('en'); Object.keys(c.resolvedOptions()).join() === 'locale,usage,sensitivity,ignorePunctuation,collation,numeric,caseFirst' && !c.resolvedOptions().numeric && c.resolvedOptions().caseFirst === 'false'",
        "let c=new Intl.Collator('en'); let d=Object.getOwnPropertyDescriptor(Intl.Collator.prototype,'compare'); typeof d.get === 'function' && !d.enumerable && d.configurable && d.set === undefined && c.resolvedOptions.length === 0",
        "let c=new Intl.Collator('en'); let f=c.compare; let caught=false; try {c.compare=9;} catch(e){caught=e instanceof TypeError;} caught && f === c.compare && f.call(null,'a','b') < 0",
        "let c=new Intl.Collator('en'); c.compare(undefined,'undefined') === 0 && c.compare(2n,'2') === 0 && c.compare(null,'null') === 0",
        "let log=''; let caught=false; try {new Intl.Collator('en').compare({toString(){log+='a';throw 7;}},{toString(){log+='b';return 'b';}});} catch(e){caught=e === 7;} caught && log === 'a'",
        "let log=''; let caught=false; try {new Intl.Collator('en',{sensitivity:{toString(){log+='s';return 'bad';}},get numeric(){log+='n';return true;}});} catch(e){caught=e instanceof RangeError;} caught && log === 's'",
        "let calls=0; new Intl.Collator('en',{numeric:{valueOf(){calls++;return 0;}},ignorePunctuation:{toString(){calls++;return '';}}}).resolvedOptions().numeric && calls === 0",
    ] {
        assert_eq!(eval(source), ValueView::Bool(true), "{source}");
    }
    for call in [
        "Intl.Collator.prototype.resolvedOptions.call({})",
        "Object.getOwnPropertyDescriptor(Intl.Collator.prototype,'compare').get.call({})",
        "new Intl.Collator('en').compare(Symbol(),'a')",
        "new Intl.Collator('en',null)",
        "new Intl.Collator('en',5)",
        "new Intl.Collator.supportedLocalesOf('en')",
    ] {
        let source = format!(
            "let caught=false; try {{{call};}} catch(e) {{caught=e instanceof TypeError;}} caught"
        );
        assert_eq!(eval(&source), ValueView::Bool(true), "{source}");
    }
    for options in [
        "{usage:'bad'}",
        "{caseFirst:'bad'}",
        "{localeMatcher:'bad'}",
        "{collation:'a!'}",
    ] {
        let source = format!(
            "let caught=false; try {{new Intl.Collator('en',{options});}} catch(e) {{caught=e instanceof RangeError;}} caught"
        );
        assert_eq!(eval(&source), ValueView::Bool(true), "{source}");
    }
}

/// Synthetic fixture, not light-cljs output. Every JavaScript callback waits;
/// the native state and bound callable survive collection and thread migration.
#[test]
fn intl_collator_callbacks_wait_collect_and_migrate() {
    let rt = Runtime::new();
    let source = r#"
        let log='';
        let c=new Intl.Collator({get length(){log+='l';__rt.need(1,1);return 1;},get 0(){log+='t';__rt.need(2,1);return 'EN';}},
            {get sensitivity(){log+='s';__rt.need(3,1);return {toString(){log+='v';__rt.need(4,1);return 'variant';}};}});
        let f=c.compare; let same=f === c.compare; c=null;
        let result=f({toString(){log+='a';__rt.need(5,1);return 'a';}}, {toString(){log+='b';__rt.need(6,1);return 'b';}});
        same && result < 0 && log === 'ltsvab'
    "#;
    let mut task = rt.spawn(&exe(source), &[]).unwrap();
    let mut waited = 0;
    for _ in 0..2500 {
        let (next, outcome) = std::thread::spawn(move || {
            let outcome = task
                .run_with(Budget::instructions(1), &mut NoEffects)
                .unwrap();
            (task, outcome)
        })
        .join()
        .unwrap();
        task = next;
        match outcome {
            RunOutcome::Yielded => {
                rt.collect().unwrap();
            }
            RunOutcome::Waiting(_) => {
                waited += 1;
                assert!(waited <= 6);
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
            RunOutcome::Complete(value) => {
                assert_eq!(value.view(), ValueView::Bool(true));
                assert_eq!(waited, 6);
                return;
            }
            other => panic!("{other:?}"),
        }
    }
    panic!("Collator continuation did not finish");
}

/// Synthetic fixtures, not light-cljs output. Equal long rope inputs force
/// complete traversal, normalization, key comparison and long numeric scans.
#[test]
fn intl_collator_long_ropes_are_budgeted() {
    for numeric in [false, true] {
        let rt = Runtime::new();
        let mut rope = clj_quickjs::JsString::from("x");
        for _ in 0..3000 {
            rope = rope.concat(&clj_quickjs::JsString::from("0")).unwrap();
        }
        rope = rope
            .concat(&clj_quickjs::JsString::from("2a\u{315}\u{300}"))
            .unwrap();
        let a = rt.root(Primitive::String(rope.clone()));
        let b = rt.root(Primitive::String(rope));
        let code = compile_function(&format!("function f(a,b){{return new Intl.Collator('en',{{numeric:{numeric}}}).compare(a,b) === 0;}}")).unwrap();
        let mut task = rt.spawn(&code, &[&a, &b]).unwrap();
        let mut yields = 0;
        let mut complete = false;
        for _ in 0..4000 {
            let (next, outcome) = std::thread::spawn(move || {
                let outcome = task
                    .run_with(Budget::instructions(1), &mut NoEffects)
                    .unwrap();
                (task, outcome)
            })
            .join()
            .unwrap();
            task = next;
            match outcome {
                RunOutcome::Yielded => {
                    yields += 1;
                    rt.collect().unwrap();
                }
                RunOutcome::Complete(value) => {
                    assert_eq!(value.view(), ValueView::Bool(true));
                    assert!(yields > 200, "{yields} yields with numeric={numeric}");
                    complete = true;
                    break;
                }
                other => panic!("{other:?}"),
            }
        }
        assert!(complete);
    }
}

/// Synthetic fixtures, not light-cljs output.
#[test]
fn intl_collator_ownership_and_host_default() {
    let rt = Runtime::new();
    let mut host = clj_quickjs::host::sim::SimHost::new(&rt);
    host.set_default_locale("fr");
    let task = rt
        .spawn(
            &exe("new Intl.Collator().resolvedOptions().locale === 'fr'"),
            &[],
        )
        .unwrap();
    assert!(
        matches!(task.run_with(Budget::UNLIMITED, &mut host).unwrap(), RunOutcome::Complete(v) if v.view() == ValueView::Bool(true))
    );
    for source in [
        "new Intl.Collator('en')",
        "Object.freeze(new Intl.Collator('en'))",
    ] {
        let RunOutcome::Complete(value) = rt.spawn(&exe(source), &[]).unwrap().run().unwrap()
        else {
            panic!("Collator construction");
        };
        assert!(matches!(
            rt.publish(&value),
            Err(clj_quickjs::PublishError::NotShareable { .. })
        ));
        for body in [
            "value.compare",
            "Intl.Collator.prototype.resolvedOptions.call(value)",
        ] {
            let code = compile_function(&format!("function f(value){{return {body};}}")).unwrap();
            assert!(
                matches!(rt.spawn(&code, &[&value]).unwrap().run().unwrap(), RunOutcome::Failed(e) if matches!(e.kind(), VmErrorKind::ForeignObject { .. }))
            );
        }
    }
}
