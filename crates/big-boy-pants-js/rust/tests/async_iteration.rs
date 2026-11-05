//! Synthetic fixtures, not light-cljs output.
use big_boy_pants_js::frontend::compile_script;
use big_boy_pants_js::host::sim::SimHost;
use big_boy_pants_js::{Budget, RunOutcome, Runtime, ValueView};

fn run(source: &str) {
    let rt = Runtime::new();
    let source = format!(
        "function assert(a, b) {{ if (a !== b) throw new Error('assert: ' + a + ' !== ' + b); }}\nlet fixtureDone = false;\n{source}\nos.setTimeout(() => assert(fixtureDone, true), 0);\nundefined;"
    );
    let code = compile_script("async-iteration.js", &source).expect("fixture compiles");
    let task = rt.spawn(&code, &[]).unwrap();
    task.install_sim_host_bindings().unwrap();
    let mut host = SimHost::new(&rt);
    let mut yields = 0;
    for _ in 0..30_000 {
        match task.run_with(Budget::instructions(3), &mut host).unwrap() {
            RunOutcome::Yielded => {
                yields += 1;
                rt.collect().unwrap();
            }
            RunOutcome::Complete(value) => {
                assert_eq!(value.view(), ValueView::Undefined);
                assert!(yields > 0);
                assert!(
                    task.unhandled_rejections().entries.is_empty(),
                    "{:?}",
                    task.unhandled_rejections()
                );
                return;
            }
            other => panic!("fixture failed: {other:?}"),
        }
    }
    panic!("fixture exceeded instruction bound");
}

#[test]
fn for_await_sync_fallback_and_async_iterators() {
    run(r#"
        async function check() {
            assert(typeof Symbol.asyncIterator, 'symbol');
            let sum = 0;
            for await (const x of [1, Promise.resolve(2)]) sum += x;
            assert(sum, 3);
            let log = '', count = 0;
            let iterable = {
                [Symbol.asyncIterator]() { log += 'i'; return this; },
                [Symbol.iterator]() { throw 'wrong protocol'; },
                get next() {
                    log += 'n';
                    return function() {
                        assert(arguments.length, 0);
                        return Promise.resolve({value: ++count, done: count > 2});
                    };
                }
            };
            sum = 0;
            for await (let x of iterable) { sum += x; }
            assert(sum, 3); assert(log, 'in');
            async function* values() { yield 4; yield Promise.resolve(5); }
            for await (const x of values()) sum += x;
            assert(sum, 12);
        }
        check().then(() => { fixtureDone = true; }, e => { fixtureDone = true; os.setTimeout(() => { throw e; }, 0); });
    "#);
}

#[test]
fn for_await_using_disposes_each_iteration() {
    run(r#"
        async function check() {
            let log = '';
            function resource(n) { return { n, [Symbol.asyncDispose]() {
                log += 'd' + n;
                return Promise.resolve().then(() => { log += 'a' + n; });
            } }; }
            for (await using x of [resource(1), resource(2)]) {
                log += 'b' + x.n;
                if (x.n === 1) continue;
            }
            assert(log, 'b1d1a1b2d2a2');
            log = '';
            async function* values() { try { yield resource(3); } finally { log += 'c'; } }
            for await (await using x of values()) { log += 'b' + x.n; break; }
            assert(log, 'b3d3a3c');
        }
        check().then(() => { fixtureDone = true; }, e => { fixtureDone = true; os.setTimeout(() => { throw e; }, 0); });
    "#);
}

#[test]
fn for_await_close_and_disposal_on_abrupt_exits() {
    run(r#"
        async function check() {
            let log = '';
            function iterator() { return {
                [Symbol.asyncIterator]() { return this; },
                next() { return Promise.resolve({value: 1, done: false}); },
                return() {
                    assert(arguments.length, 0);
                    log += 'c';
                    return {then(resolve) { log += 'a'; resolve({}); }};
                }
            }; }
            function resource() { return { [Symbol.asyncDispose]() {
                log += 'd'; return Promise.resolve().then(() => { log += 'e'; });
            } }; }
            async function consume(mode) {
                outer: for (let n = 0; n < 1; n++) {
                    for await (const x of iterator()) {
                        await using r = resource();
                        log += 'b';
                        if (mode === 'break') break;
                        if (mode === 'return') return 7;
                        if (mode === 'throw') throw 8;
                        if (mode === 'continue') continue outer;
                        if (mode === 'outer-break') break outer;
                    }
                }
                log += 'f'; return 9;
            }
            for (const mode of ['break', 'return', 'throw', 'continue', 'outer-break']) {
                log = '';
                try {
                    let value = await consume(mode);
                    assert(value, mode === 'return' ? 7 : 9);
                } catch(e) { assert(mode, 'throw'); assert(e, 8); }
                assert(log, mode === 'return' || mode === 'throw' ? 'bdeca' : 'bdecaf');
            }
            // The resource is the loop binding in this variant.
            let it = iterator(); it.next = () => ({value: resource(), done: false});
            log = '';
            async function bound() { for await (await using x of it) return 10; }
            assert(await bound(), 10); assert(log, 'deca');
        }
        check().then(() => { fixtureDone = true; }, e => { fixtureDone = true; os.setTimeout(() => { throw e; }, 0); });
    "#);
}

#[test]
fn for_await_close_errors_and_protocol_failures() {
    run(r#"
        async function check() {
            let closes = 0;
            let it = {
                [Symbol.asyncIterator]() { return this; },
                next() { return {value: 1, done: false}; },
                return() { closes++; return Promise.reject(12); }
            };
            async function consume(mode) {
                for await (const x of it) {
                    if (mode === 'throw') throw 11;
                    if (mode === 'return') return 1;
                    break;
                }
            }
            for (const mode of ['break', 'return', 'throw']) {
                try { await consume(mode); assert(false, true); }
                catch(e) { assert(e, mode === 'throw' ? 11 : 12); }
            }
            assert(closes, 3);
            it.return = () => Promise.resolve(1);
            try { await consume('break'); assert(false, true); }
            catch(e) { assert(e instanceof TypeError, true); }
            try { await consume('throw'); assert(false, true); }
            catch(e) { assert(e, 11); }
            it.return = 1;
            try { await consume('break'); assert(false, true); }
            catch(e) { assert(e instanceof TypeError, true); }
            Object.defineProperty(it, 'return', {configurable: true, get() { throw 13; }});
            try { await consume('throw'); assert(false, true); } catch(e) { assert(e, 11); }
            try { await consume('break'); assert(false, true); } catch(e) { assert(e, 13); }
            Object.defineProperty(it, 'return', {value() { closes++; return {}; }});
            closes = 0;
            for (const next of [
                () => { throw 14; },
                () => Promise.reject(14),
                () => Promise.resolve(1),
                () => ({get done() { throw 14; }}),
                () => ({done: false, get value() { throw 14; }})
            ]) {
                it.next = next;
                try { for await (const x of it) {} assert(false, true); }
                catch(e) { assert(e === 14 || e instanceof TypeError, true); }
                assert(closes, 0);
            }
            it.next = () => ({done: true, get value() { throw 'must not read'; }});
            for await (const x of it) { throw 'must not enter'; }
            assert(closes, 0);
            it.next = () => ({done: false, value: null});
            try { for await (const {x} of it) {} assert(false, true); }
            catch(e) { assert(e instanceof TypeError, true); }
            assert(closes, 1);
        }
        check().then(() => { fixtureDone = true; }, e => { fixtureDone = true; os.setTimeout(() => { throw e; }, 0); });
    "#);
}

#[test]
fn for_await_bindings_values_and_sync_adapter() {
    run(r#"
        async function check() {
            let closures = [], target = 0, total = 0;
            for await (let [x, ...rest] of [[1, 2], [3, 4, 5]]) {
                closures.push(() => x); total += rest.length;
            }
            assert(closures[0](), 1); assert(closures[1](), 3); assert(total, 3);
            for await (var x of [6, 7]) target += x;
            assert(x, 7); assert(target, 13);
            for await (target of [8]) {} assert(target, 8);
            let async = 0;
            for await (async of [9]) {} assert(async, 9);
            try { for await (let x of [x]) {} assert(false, true); }
            catch(e) { assert(e instanceof ReferenceError, true); }
            let value = Promise.resolve(20), closes = 0;
            let it = {
                [Symbol.asyncIterator]() { return this; },
                next() { return {then(resolve) { resolve({value, done: false}); }}; },
                return() { closes++; return {}; }
            };
            for await (const x of it) { assert(x, value); break; }
            assert(closes, 1);
            // A null async method selects sync fallback; its values are awaited.
            let count = 0, nextReads = 0;
            it = {
                [Symbol.asyncIterator]: null,
                [Symbol.iterator]() { return this; },
                get next() { nextReads++; return function() {
                    assert(arguments.length, 0);
                    return {value: Promise.resolve(++count), done: count > 1};
                }; },
                return() {
                    assert(arguments.length, 0);
                    closes++; return {value: Promise.resolve(0), done: true};
                }
            };
            for await (const x of it) { assert(x, 1); break; }
            assert(nextReads, 1); assert(closes, 2);
            it[Symbol.asyncIterator] = 1;
            try { for await (const x of it) {} assert(false, true); }
            catch(e) { assert(e instanceof TypeError, true); }
            assert(nextReads, 1);
            // Sync iterator values reject through the adapter and close it.
            it[Symbol.asyncIterator] = null;
            Object.defineProperty(it, 'next', {value() { return {value: Promise.reject(21), done: false}; }});
            try { for await (const x of it) {} assert(false, true); }
            catch(e) { assert(e, 21); }
            assert(closes, 3);
        }
        check().then(() => { fixtureDone = true; }, e => { fixtureDone = true; os.setTimeout(() => { throw e; }, 0); });
    "#);
}

#[test]
fn for_await_using_disposal_errors_and_sync_fallback() {
    run(r#"
        async function check() {
            let log = '';
            let it = {
                [Symbol.asyncIterator]() { return this; },
                next() { return {value: { [Symbol.asyncDispose]() {
                    log += 'd'; return Promise.reject(31);
                } }, done: false}; },
                return() { log += 'c'; return Promise.reject(32); }
            };
            try { for await (await using x of it) { throw 30; } assert(false, true); }
            catch(e) {
                assert(e instanceof SuppressedError, true);
                assert(e.error, 31); assert(e.suppressed, 30);
            }
            assert(log, 'dc');
            log = '';
            try { for await (await using x of it) { break; } assert(false, true); }
            catch(e) { assert(e, 31); }
            assert(log, 'dc');
            let resource = {[Symbol.dispose]() { log += 's'; }};
            for (await using x of [resource, null, undefined]) { assert(x === resource || x == null, true); }
            assert(log, 'dcs');
            for (await using of of [resource]) { assert(of, resource); }
            assert(log, 'dcss');
            it.next = () => ({value: 1, done: false});
            try { for await (await using x of it) {} assert(false, true); }
            catch(e) { assert(e instanceof TypeError, true); }
            assert(log, 'dcssc');
        }
        check().then(() => { fixtureDone = true; }, e => { fixtureDone = true; os.setTimeout(() => { throw e; }, 0); });
    "#);
}

#[test]
fn for_await_generator_consumer_resumes_and_closes() {
    run(r#"
        async function check() {
            let log = '';
            function source(failClose) { return {
                n: 0,
                [Symbol.asyncIterator]() { return this; },
                async next() {
                    let n = ++this.n;
                    log += 'n' + n; await null;
                    return {done: n > 2, value: { n, [Symbol.asyncDispose]() {
                        log += 'd' + n;
                        return Promise.resolve().then(() => { log += 'a' + n; });
                    } }};
                },
                async return() {
                    assert(arguments.length, 0);
                    log += 'c'; await null; log += 'z';
                    if (failClose) throw 11;
                    return {};
                }
            }; }
            async function* consume(failClose) {
                try {
                    for await (await using x of source(failClose)) {
                        log += 'b' + x.n; yield x.n;
                    }
                } finally { log += 'f'; }
            }
            let it = consume(false);
            let r = await it.next(); assert(r.value, 1); assert(r.done, false);
            assert(log, 'n1b1');
            r = await it.next(); assert(r.value, 2); assert(r.done, false);
            assert(log, 'n1b1d1a1n2b2');
            r = await it.return(Promise.resolve(9));
            assert(r.value, 9); assert(r.done, true);
            assert(log, 'n1b1d1a1n2b2d2a2czf');
            assert((await it.next()).done, true);
            log = ''; it = consume(true);
            assert((await it.next()).value, 1);
            try { await it.throw(10); assert(false, true); } catch(e) { assert(e, 10); }
            assert(log, 'n1b1d1a1czf');
            assert((await it.next()).done, true);
            log = ''; it = consume(false);
            assert((await it.next()).value, 1);
            assert((await it.next()).value, 2);
            r = await it.next(); assert(r.value, undefined); assert(r.done, true);
            assert(log, 'n1b1d1a1n2b2d2a2n3f');
        }
        check().then(() => { fixtureDone = true; }, e => { fixtureDone = true; os.setTimeout(() => { throw e; }, 0); });
    "#);
}
