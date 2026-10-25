//! Synthetic fixtures, not light-cljs output.
use clj_quickjs::frontend::compile_script;
use clj_quickjs::host::sim::SimHost;
use clj_quickjs::{Budget, RunOutcome, Runtime, ValueView};

fn run(source: &str) {
    let rt = Runtime::new();
    let source = format!(
        "function assert(a, b) {{ if (a !== b) throw new Error('assert: ' + a + ' !== ' + b); }}\nlet fixtureDone = false;\n{source}\nos.setTimeout(() => assert(fixtureDone, true), 0);\nundefined;"
    );
    let code = compile_script("async-generators.js", &source).expect("fixture compiles");
    let task = rt.spawn(&code, &[]).unwrap();
    task.install_sim_host_bindings().unwrap();
    let mut host = SimHost::new(&rt);
    for _ in 0..30_000 {
        match task.run_with(Budget::instructions(3), &mut host).unwrap() {
            RunOutcome::Yielded => {
                rt.collect().unwrap();
            }
            RunOutcome::Complete(value) => {
                assert_eq!(value.view(), ValueView::Undefined);
                assert!(
                    task.unhandled_rejections().entries.is_empty(),
                    "{:?}",
                    task.unhandled_rejections()
                );
                drop(value);
                drop(task);
                rt.collect().unwrap();
                assert_eq!(rt.heap_usage().objects.generator, 0);
                assert_eq!(rt.heap_usage().objects.async_state, 0);
                return;
            }
            other => panic!("fixture failed: {other:?}"),
        }
    }
    panic!("fixture exceeded instruction bound");
}

#[test]
fn async_generator_queue_await_and_collection() {
    run(r#"
        assert(typeof Symbol.asyncIterator, 'symbol');
        let log = '';
        async function* g() {
            log += 'a';
            let n = yield Promise.resolve(1);
            log += n;
            yield await Promise.resolve(2);
            return Promise.resolve(3);
        }
        let it = g();
        assert(it[Symbol.asyncIterator](), it);
        assert(it[Symbol.asyncIterator].name, '[Symbol.asyncIterator]');
        assert(it[Symbol.asyncIterator].length, 0);
        assert(log, '');
        let a = it.next(); let b = it.next('b'); let c = it.next(); let d = it.next();
        assert(a instanceof Promise, true);
        async function check() {
            let r = [await a, await b, await c, await d];
            assert(r[0].value, 1); assert(r[0].done, false);
            assert(r[1].value, 2); assert(r[1].done, false);
            assert(r[2].value, 3); assert(r[2].done, true);
            assert(r[3].value, undefined); assert(r[3].done, true);
            assert(log, 'ab');
        }
        check().then(() => { fixtureDone = true; });
    "#);
}

#[test]
fn async_generator_return_throw_and_rejection() {
    run(r#"
        async function check() {
            let entered = false;
            async function* never() { entered = true; yield 0; }
            let fresh = never();
            let r = await fresh.return(Promise.resolve(7));
            assert(r.value, 7); assert(r.done, true); assert(entered, false);
            try { await fresh.throw(8); assert(false, true); } catch(e) { assert(e, 8); }
            let done = await fresh.next(); assert(done.done, true);
            try { await fresh.return(Promise.reject(9)); assert(false, true); } catch(e) { assert(e, 9); }
            let invalid = Object.getPrototypeOf(Object.getPrototypeOf(fresh)).next.call({});
            assert(invalid instanceof Promise, true);
            try { await invalid; assert(false, true); } catch(e) { assert(e instanceof TypeError, true); }
            let log = '';
            async function* g() {
                try {
                    try { yield 1; } catch(e) { yield e; }
                    yield Promise.reject(5);
                } catch(e) { yield e + 1; }
                finally { log += 'f'; yield 10; log += 'g'; }
            }
            let it = g();
            assert((await it.next()).value, 1);
            assert((await it.throw(3)).value, 3);
            assert((await it.next()).value, 6);
            let stop = it.return(Promise.resolve(20));
            let finish = it.next();
            assert((await stop).value, 10);
            let last = await finish; assert(last.value, 20); assert(last.done, true);
            assert(log, 'fg');
            async function* rejectedReturn() {
                try { yield 1; } catch(e) { yield e; }
            }
            it = rejectedReturn(); await it.next();
            assert((await it.return(Promise.reject(30))).value, 30);
            assert((await it.next()).done, true);
        }
        check().then(() => { fixtureDone = true; });
    "#);
}

#[test]
fn async_generator_delegation_and_sync_fallback() {
    run(r#"
        async function check() {
            let log = '';
            let delegate = {
                [Symbol.asyncIterator]() { log += 'i'; return this; },
                [Symbol.iterator]() { throw 'sync must not be selected'; },
                get next() {
                    log += 'n';
                    return function(x) { log += 'N'; return Promise.resolve({ value: x === undefined ? 1 : x, done: false }); };
                },
                throw(x) { log += 'T'; return Promise.resolve({value: x + 1, done: false}); },
                return(x) { log += 'R'; assert(x, 8); return Promise.resolve({value: 9, done: true}); }
            };
            async function* g() { return yield* delegate; }
            let it = g();
            assert((await it.next()).value, 1);
            assert((await it.next(4)).value, 4);
            assert((await it.throw(5)).value, 6);
            let r = await it.return(Promise.resolve(8));
            assert(r.value, 9); assert(r.done, true); assert(log, 'inNNTR');
            async function* fallback() { return yield* [Promise.resolve(10), Promise.resolve(11)]; }
            it = fallback();
            assert((await it.next()).value, 10);
            assert((await it.next()).value, 11);
            assert((await it.next()).done, true);
            async function* nested() { yield* fallback(); }
            it = nested(); assert((await it.next()).value, 10);
            r = await it.return(12); assert(r.value, 12); assert(r.done, true);
        }
        check().then(() => { fixtureDone = true; });
    "#);
}

#[test]
fn async_generator_sync_adapter_close_and_errors() {
    run(r#"
        async function check() {
            let closed = 0;
            let sync = {
                [Symbol.iterator]() { return this; },
                next() { return {value: Promise.reject(17), done: false}; },
                return() { closed++; throw 'ignored'; }
            };
            async function* g() { yield* sync; }
            try { await g().next(); assert(false, true); } catch(e) { assert(e, 17); }
            assert(closed, 1);
            sync.next = function() { return {value: Promise.resolve(18), done: false}; };
            sync.return = function() { closed++; return {}; };
            let it = g(); assert((await it.next()).value, 18);
            try { await it.throw(19); assert(false, true); } catch(e) { assert(e instanceof TypeError, true); }
            assert(closed, 2);
            let async = { [Symbol.asyncIterator]() { return this; }, next() { return Promise.resolve(1); } };
            async function* bad() { yield* async; }
            try { await bad().next(); assert(false, true); } catch(e) { assert(e instanceof TypeError, true); }
        }
        check().then(() => { fixtureDone = true; });
    "#);
}

#[test]
fn async_generator_reentry_thenables_and_queued_roots() {
    run(r#"
        let queued;
        let it;
        async function* g() {
            queued = it.next({n: 22});
            let received = yield {then(resolve) { resolve({n: 21}); }};
            assert(received.n, 22);
            yield received;
            return 23;
        }
        it = g();
        async function check() {
            let first = it.next();
            let third = it.next();
            assert((await first).value.n, 21);
            assert((await queued).value.n, 22);
            let last = await third;
            assert(last.value, 23); assert(last.done, true);
            let fresh = (async function*() { yield 0; })();
            let r = fresh.return({then(resolve) { resolve(24); }});
            let t = fresh.throw(25).catch(e => e);
            let n = fresh.next();
            assert((await r).value, 24); assert(await t, 25); assert((await n).done, true);
            let empty = (async function*() {})();
            assert((await empty.next()).done, true);
            let parameters = 0;
            async function* defaults(x = ++parameters) { yield x; }
            let d = defaults(); assert(parameters, 1); assert((await d.next()).value, 1);
            let syncThrown = false;
            async function* badDefault(x = (() => { throw 26; })()) { yield x; }
            try { badDefault(); } catch(e) { syncThrown = e === 26; }
            assert(syncThrown, true);
            try { new defaults(); assert(false, true); } catch(e) { assert(e instanceof TypeError, true); }
        }
        check().then(() => { fixtureDone = true; });
    "#);
}

#[test]
fn async_generator_methods_and_async_disposal() {
    run(r#"
        async function check() {
            let object = {n: 31, async *values() { yield await this.n; }};
            let it = object.values();
            assert(it[Symbol.iterator], undefined);
            assert(it[Symbol.toStringTag], 'AsyncGenerator');
            assert((await it.next()).value, 31);
            class Base { async *values() { yield 32; } }
            class Derived extends Base { async *values() { yield* super.values(); yield this.n; } }
            let instance = new Derived(); instance.n = 33;
            it = instance.values();
            assert((await it.next()).value, 32);
            assert((await it.next()).value, 33);
            let disposed = false;
            async function* resources() {
                await using resource = { [Symbol.asyncDispose]() { return Promise.resolve().then(() => { disposed = true; }); } };
                yield 34;
            }
            it = resources(); assert((await it.next()).value, 34); assert(disposed, false);
            let last = await it.return(35);
            assert(last.value, 35); assert(last.done, true); assert(disposed, true);
        }
        check().then(() => { fixtureDone = true; });
    "#);
}

#[test]
fn async_generator_delegated_return_rejection_calls_throw() {
    run(r#"
        async function check() {
            let thrown;
            let delegate = {
                [Symbol.asyncIterator]() { return this; },
                next() { return {value: 1, done: false}; },
                throw(e) { thrown = e; return {value: 9, done: false}; },
                return() { throw 'return must not run'; }
            };
            async function* g() { yield* delegate; }
            let it = g(); await it.next();
            let result = await it.return(Promise.reject(7));
            assert(thrown, 7); assert(result.value, 9); assert(result.done, false);
            assert((await it.next()).value, 1);
        }
        check().then(() => { fixtureDone = true; });
    "#);
}

#[test]
fn async_generator_delegated_return_preserves_value() {
    run(r#"
        async function check() {
            let called = false;
            let sentinel = {then() { called = true; }};
            let delegate = {
                [Symbol.asyncIterator]() { return this; },
                next() { return {value: 1, done: false}; },
                return() { return {value: sentinel, done: true}; }
            };
            async function* g() { yield* delegate; }
            let it = g(); await it.next();
            let result = await it.return(0);
            assert(result.value, sentinel); assert(result.done, true); assert(called, false);
        }
        check().then(() => { fixtureDone = true; });
    "#);
}

#[test]
fn async_generator_sync_adapter_preserves_arguments() {
    run(r#"
        async function check() {
            let log = '', expected;
            let sync = {
                [Symbol.iterator]() { return this; },
                next(value) {
                    assert(this, sync); assert(arguments.length, 1); assert(value, expected);
                    log += 'n'; return {value: 1, done: false};
                },
                return(value) {
                    assert(this, sync); assert(arguments.length, 1); assert(value, expected);
                    log += 'r'; return {value: 2, done: true};
                },
                throw(value) {
                    assert(this, sync); assert(arguments.length, 1); assert(value, expected);
                    log += 't'; return {value: 3, done: false};
                }
            };
            async function* g() { return yield* sync; }
            let it = g();
            // yield* supplies undefined even for the initial next request.
            assert((await it.next()).value, 1);
            assert((await it.next(undefined)).value, 1);
            expected = {tag: 'next'};
            assert((await it.next(expected)).value, 1);
            expected = undefined;
            assert((await it.throw(undefined)).value, 3);
            expected = {tag: 'throw'};
            assert((await it.throw(expected)).value, 3);
            expected = undefined;
            let r = await it.return(undefined);
            assert(r.value, 2); assert(r.done, true); assert(log, 'nnnttr');
            log = ''; it = g();
            assert((await it.next()).value, 1);
            expected = {tag: 'return'};
            r = await it.return(expected);
            assert(r.value, 2); assert(r.done, true); assert(log, 'nr');
        }
        check().then(() => { fixtureDone = true; });
    "#);
}
