//! Synthetic JavaScript fixtures, not light-cljs output.
// The opening rejection-counter case adapts QuickJS tests/test_std.js,
// test_async_promise_rejection, revision 7d1c13c5d8f8c6b32893786ed35cdcebd88194d2.
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
use clj_quickjs::frontend::compile_script;
use clj_quickjs::{Budget, Host, HostCall, Primitive, RunOutcome, Runtime, ValueView};

#[derive(Default)]
struct Effects(Vec<Primitive>);
impl Host for Effects {
    fn effect(&mut self, call: HostCall) -> Result<Primitive, String> {
        self.0.push(call.arg);
        Ok(Primitive::Undefined)
    }
}
fn run(source: &str) -> (ValueView, Vec<Primitive>) {
    let rt = Runtime::new();
    let task = rt
        .spawn(&compile_script("promise.js", source).unwrap(), &[])
        .unwrap();
    let mut host = Effects::default();
    let out = task.run_with(Budget::UNLIMITED, &mut host).unwrap();
    let RunOutcome::Complete(value) = out else {
        panic!("{out:?}")
    };
    (value.view(), host.0)
}
#[test]
fn promise_then_catch_jobs() {
    let (result, effects) = run(r#"
        let counter = 0;
        let p1 = Promise.reject(); let p2 = Promise.reject(); let p3 = Promise.resolve();
        p1.catch(() => counter++); p2.catch(() => counter++); p3.then(() => counter++);
        Promise.resolve().then(() => __rt.effect(0, counter));
        Promise.resolve(1).then(x => { __rt.effect(0, 'a1'); return x + 1; })
          .then(x => { __rt.effect(0, 'a2'); __rt.effect(0, x); throw 7; })
          .catch(x => __rt.effect(0, x));
        Promise.resolve().then(() => __rt.effect(0, 'b1')).then(() => __rt.effect(0, 'b2'));
        let n = 0; new Promise(() => { n++; }).then(() => __rt.effect(0, 'never'));
        new Promise(() => { throw 9; }).catch(x => __rt.effect(0, x));
        new Promise((resolve, reject) => { resolve(4); resolve(5); reject(6); })
          .then(x => __rt.effect(0, x));
        let errors = 0;
        try { Promise(() => {}); } catch(e) { if (e instanceof TypeError) errors++; }
        try { new Promise(1); } catch(e) { if (e instanceof TypeError) errors++; }
        let p = Promise.resolve(1);
        __rt.effect(0, 'body');
        n === 1 && errors === 2 && Promise.resolve(p) === p;
    "#);
    assert_eq!(result, ValueView::Bool(true));
    let strings: Vec<_> = effects
        .iter()
        .filter_map(|v| match v {
            Primitive::String(s) => Some(s.to_string_lossy()),
            _ => None,
        })
        .collect();
    assert_eq!(strings, ["body", "a1", "b1", "a2", "b2"]);
    let numbers: Vec<_> = effects
        .iter()
        .filter_map(|v| match v {
            Primitive::Number(n) => Some(*n),
            _ => None,
        })
        .collect();
    assert_eq!(numbers, [3.0, 9.0, 4.0, 2.0, 7.0]);
}

fn finish(task: &clj_quickjs::Task) -> clj_quickjs::Rooted {
    let out = task.run().unwrap();
    let RunOutcome::Complete(value) = out else {
        panic!("{out:?}")
    };
    value
}
fn function(rt: &Runtime, source: &str, args: &[clj_quickjs::Rooted]) -> clj_quickjs::Task {
    rt.spawn(
        &clj_quickjs::frontend::compile_function(source).unwrap(),
        &args.iter().collect::<Vec<_>>(),
    )
    .unwrap()
}
fn satisfy(rt: &Runtime, task: &clj_quickjs::Task) {
    let request = task.take_producer_requests().pop().unwrap();
    let value = rt.root(Primitive::Number(8.0));
    let wakes = rt
        .complete(
            request.key(),
            request.producer(),
            clj_quickjs::Completion::Ready(&value),
        )
        .unwrap();
    assert_eq!(wakes.task_ids(), [task.id()]);
}

#[test]
fn promise_reaction_waits_and_resumes() {
    let rt = Runtime::new();
    let task = function(
        &rt,
        r#"function body() {
        Promise.resolve(1).then(x => { __rt.effect(0, x); __rt.need(246, 1); __rt.effect(0, 3); })
          .then(() => __rt.effect(0, 5));
        Promise.resolve().then(() => __rt.effect(0, 4));
        __rt.effect(0, 0); return 42;
    }"#,
        &[],
    );
    let (tx, rx) = std::sync::mpsc::channel();
    let t = task.clone();
    let thread = std::thread::spawn(move || {
        let mut host = Effects::default();
        let out = t.run_with(Budget::UNLIMITED, &mut host).unwrap();
        tx.send((out, host)).unwrap();
    });
    let (out, mut host) = rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap();
    thread.join().unwrap();
    assert!(matches!(out, RunOutcome::Waiting(_)));
    assert_eq!(host.0, [Primitive::Number(0.0), Primitive::Number(1.0)]);
    rt.collect().unwrap();
    satisfy(&rt, &task);
    let RunOutcome::Complete(value) = task.run_with(Budget::UNLIMITED, &mut host).unwrap() else {
        panic!("complete")
    };
    assert_eq!(value.view(), ValueView::Number(42.0));
    assert_eq!(host.0, [0., 1., 3., 4., 5.].map(Primitive::Number));
    assert_eq!(task.status(), clj_quickjs::TaskStatus::Finished);
}

#[test]
fn pending_promise_reactions_are_traced() {
    let rt = Runtime::new();
    let object = rt.scope(|scope| scope.persist(scope.new_object()));
    let weak = object.downgrade();
    let task = function(
        &rt,
        r#"function body(capture) {
        const p = new Promise(() => {});
        p.then(() => capture);
        return () => p;
    }"#,
        &[object],
    );
    let closure = finish(&task);
    drop(task);
    rt.collect().unwrap();
    assert!(weak.is_live());
    assert_eq!(rt.heap_usage().objects.promise, 2);
    assert_eq!(rt.heap_usage().objects.promise_reaction, 1);
    drop(closure);
    rt.collect().unwrap();
    assert!(!weak.is_live());
    assert_eq!(rt.heap_usage().objects.promise, 0);
    assert_eq!(rt.heap_usage().objects.promise_reaction, 0);
}

#[test]
fn promise_publication_and_cross_task_settlement_do_not_run_callbacks() {
    let rt = Runtime::new();
    let maker = function(
        &rt,
        "function make() { let resolve; const p = new Promise(r => { resolve = r; }); return __rt.vector(p, resolve); }",
        &[],
    );
    let pair = finish(&maker);
    rt.publish(&pair).unwrap();
    let get = |i: u8| {
        finish(&function(
            &rt,
            &format!("function get(pair) {{ return __rt.nth(pair, {i}); }}"),
            std::slice::from_ref(&pair),
        ))
    };
    let promise = get(0);
    let resolve = get(1);
    rt.publish(&promise).unwrap();
    rt.publish(&resolve).unwrap();
    let consumer = function(
        &rt,
        r#"function consume(p) {
        let n = 40;
        p.then(x => { n += x; __rt.effect(0, n); });
        __rt.need(246, 1); return 42;
    }"#,
        &[promise],
    );
    let mut host = Effects::default();
    assert!(matches!(
        consumer.run_with(Budget::UNLIMITED, &mut host).unwrap(),
        RunOutcome::Waiting(_)
    ));
    let settler = function(
        &rt,
        "function settle(resolve) { resolve(2); return 1; }",
        &[resolve],
    );
    let (tx, rx) = std::sync::mpsc::channel();
    let thread = std::thread::spawn(move || {
        tx.send(finish(&settler).view()).unwrap();
    });
    assert_eq!(
        rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap(),
        ValueView::Number(1.0)
    );
    thread.join().unwrap();
    assert!(host.0.is_empty());
    assert_eq!(consumer.status(), clj_quickjs::TaskStatus::Waiting);
    rt.collect().unwrap();
    satisfy(&rt, &consumer);
    assert!(matches!(
        consumer.run_with(Budget::UNLIMITED, &mut host).unwrap(),
        RunOutcome::Complete(_)
    ));
    assert_eq!(host.0, [Primitive::Number(42.0)]);
}

#[test]
fn promise_cancellation_releases_queued_jobs_and_runs_finally() {
    let rt = Runtime::new();
    let object = rt.scope(|scope| scope.persist(scope.new_object()));
    let weak = object.downgrade();
    let task = function(
        &rt,
        r#"function body(o) {
        Promise.resolve().then(() => { try { __rt.need(246, 1); } finally { __rt.effect(0, 1); } });
        Promise.resolve().then(() => { o; __rt.effect(0, 2); });
        return 42;
    }"#,
        &[object],
    );
    let mut host = Effects::default();
    assert!(matches!(
        task.run_with(Budget::UNLIMITED, &mut host).unwrap(),
        RunOutcome::Waiting(_)
    ));
    rt.collect().unwrap();
    assert!(weak.is_live());
    assert!(matches!(task.cancel(), clj_quickjs::Cancel::Runnable(_)));
    assert!(matches!(
        task.run_with(Budget::UNLIMITED, &mut host).unwrap(),
        RunOutcome::Cancelled
    ));
    assert_eq!(host.0, [Primitive::Number(1.0)]);
    rt.collect().unwrap();
    assert!(!weak.is_live());
}

#[test]
fn promise_resolution_identity_throwers_and_getters() {
    let (result, effects) = run(r#"
        let resolve; const p = new Promise(r => { resolve = r; });
        resolve(p); p.catch(e => __rt.effect(0, e instanceof TypeError));
        Promise.resolve(2).then(5).then(x => __rt.effect(0, x));
        Promise.reject(3).then(null, 7).catch(x => __rt.effect(0, x));
        new Promise((r, j) => { r(4); throw 5; }).then(x => __rt.effect(0, x));
        const object = {value: 6, then: 1};
        Promise.resolve(object).then(x => __rt.effect(0, x === object));
        Promise.resolve({get then() { throw 9; }}).catch(x => __rt.effect(0, x));
        const generic = {get then() { return function(a, b) { return b(11); }; }};
        Promise.prototype.catch.call(generic, x => x + 1) === 12;
    "#);
    assert_eq!(result, ValueView::Bool(true));
    assert_eq!(
        effects,
        [
            Primitive::Bool(true),
            Primitive::Number(4.),
            Primitive::Bool(true),
            Primitive::Number(9.),
            Primitive::Number(2.),
            Primitive::Number(3.)
        ]
    );
}

#[test]
fn thenable_adoption_fails_explicitly() {
    for source in [
        "Promise.resolve({then(resolve) { resolve(1); }});",
        "new Promise(resolve => resolve({get then() { return function() {}; }}));",
        "Promise.resolve(1).then(() => Promise.resolve(2));",
    ] {
        let rt = Runtime::new();
        let task = rt
            .spawn(&compile_script("thenable.js", source).unwrap(), &[])
            .unwrap();
        let RunOutcome::Failed(error) = task.run().unwrap() else {
            panic!("must fail explicitly")
        };
        assert_eq!(error.kind(), clj_quickjs::VmErrorKind::UnsupportedOperand);
        assert!(error.message().contains("thenable adoption"));
    }
}

#[test]
fn promises_and_resolvers_are_shareable_with_task_local_results_guarded() {
    let rt = Runtime::new();
    let maker = function(
        &rt,
        "function make() { return Promise.resolve({value: 1}); }",
        &[],
    );
    let promise = finish(&maker);
    rt.publish(&promise).unwrap();
    let consumer = function(
        &rt,
        "function consume(p) { p.then(x => __rt.effect(0, 'bad')).catch(e => __rt.effect(0, e instanceof TypeError)); return 42; }",
        &[promise],
    );
    let mut host = Effects::default();
    assert!(matches!(
        consumer.run_with(Budget::UNLIMITED, &mut host).unwrap(),
        RunOutcome::Complete(_)
    ));
    assert_eq!(host.0, [Primitive::Bool(true)]);
}

#[test]
fn promise_executors_and_then_getters_wait_without_replay() {
    for source in [
        "new Promise(resolve => { __rt.effect(0, 1); resolve(__rt.need(246, 1)); }).then(x => __rt.effect(0, x)); 42;",
        "Promise.resolve({get then() { __rt.effect(0, 1); __rt.need(246, 1); return undefined; }}).then(() => __rt.effect(0, 8)); 42;",
        "Promise.prototype.catch.call({get then() { __rt.effect(0, 1); __rt.need(246, 1); return function(a, b) { b(8); }; }}, x => __rt.effect(0, x)); 42;",
    ] {
        let rt = Runtime::new();
        let task = rt
            .spawn(&compile_script("executor-wait.js", source).unwrap(), &[])
            .unwrap();
        let mut host = Effects::default();
        assert!(matches!(
            task.run_with(Budget::UNLIMITED, &mut host).unwrap(),
            RunOutcome::Waiting(_)
        ));
        assert_eq!(host.0, [Primitive::Number(1.)]);
        rt.collect().unwrap();
        satisfy(&rt, &task);
        assert!(matches!(
            task.run_with(Budget::UNLIMITED, &mut host).unwrap(),
            RunOutcome::Complete(_)
        ));
        assert_eq!(host.0, [Primitive::Number(1.), Primitive::Number(8.)]);
    }
}

#[test]
fn racing_resolution_and_registration_deliver_once() {
    for _ in 0..32 {
        let rt = Runtime::new();
        let pair = finish(&function(
            &rt,
            "function make() { let resolve; let p = new Promise(r => { resolve = r; }); return __rt.vector(p, resolve); }",
            &[],
        ));
        rt.publish(&pair).unwrap();
        let consumer = function(
            &rt,
            "function consume(pair) { const p = __rt.nth(pair, 0); p.then(x => __rt.effect(0, x)); __rt.need(246, 1); return 42; }",
            std::slice::from_ref(&pair),
        );
        let settlers: Vec<_> = [1, 2]
            .into_iter()
            .map(|n| {
                function(
                    &rt,
                    &format!("function settle(pair) {{ __rt.nth(pair, 1)({n}); }}"),
                    std::slice::from_ref(&pair),
                )
            })
            .collect();
        let gate = std::sync::Arc::new(std::sync::Barrier::new(4));
        let (tx, rx) = std::sync::mpsc::channel();
        let mut threads = Vec::new();
        for task in settlers
            .into_iter()
            .chain(std::iter::once(consumer.clone()))
        {
            let tx = tx.clone();
            let gate = gate.clone();
            threads.push(std::thread::spawn(move || {
                let mut host = Effects::default();
                gate.wait();
                let outcome = task.run_with(Budget::UNLIMITED, &mut host).unwrap();
                tx.send((outcome, host)).unwrap();
            }));
        }
        gate.wait();
        let mut host = None;
        for _ in 0..3 {
            let (outcome, effects) = rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap();
            match outcome {
                RunOutcome::Waiting(_) => {
                    assert!(effects.0.is_empty());
                    host = Some(effects);
                }
                RunOutcome::Complete(_) => assert!(effects.0.is_empty()),
                other => panic!("{other:?}"),
            }
        }
        for thread in threads {
            thread.join().unwrap();
        }
        rt.collect().unwrap();
        let mut host = host.unwrap();
        satisfy(&rt, &consumer);
        assert!(matches!(
            consumer.run_with(Budget::UNLIMITED, &mut host).unwrap(),
            RunOutcome::Complete(_)
        ));
        assert_eq!(host.0.len(), 1);
        assert!(host.0 == [Primitive::Number(1.)] || host.0 == [Primitive::Number(2.)]);
    }
}

#[test]
fn promise_budgeting_does_not_replay_executor_or_reactions() {
    let rt = Runtime::new();
    let task = function(
        &rt,
        "function body() { new Promise(r => { __rt.effect(0, 1); r(2); }).then(x => { __rt.effect(0, x); return 3; }).then(x => __rt.effect(0, x)); return 42; }",
        &[],
    );
    let mut host = Effects::default();
    for _ in 0..1000 {
        match task.run_with(Budget::instructions(1), &mut host).unwrap() {
            RunOutcome::Yielded => {
                rt.collect().unwrap();
            }
            RunOutcome::Complete(value) => {
                assert_eq!(value.view(), ValueView::Number(42.));
                assert_eq!(host.0, [1., 2., 3.].map(Primitive::Number));
                return;
            }
            out => panic!("{out:?}"),
        }
    }
    panic!("promise execution did not complete");
}

#[test]
fn settled_promise_results_and_resolvers_are_traced() {
    for source in [
        "function make(o) { const p = Promise.resolve(o); return () => p; }",
        "function make(o) { let resolve; new Promise(r => { resolve = r; r(o); }); return resolve; }",
    ] {
        let rt = Runtime::new();
        let object = rt.scope(|scope| scope.persist(scope.new_object()));
        let weak = object.downgrade();
        let maker = function(&rt, source, &[object]);
        let holder = finish(&maker);
        drop(maker);
        rt.collect().unwrap();
        assert!(weak.is_live());
        drop(holder);
        rt.collect().unwrap();
        assert!(!weak.is_live());
        assert_eq!(rt.heap_usage().objects.promise, 0);
    }
}

#[test]
fn reaction_finally_waits_before_rejecting_the_derived_promise() {
    let rt = Runtime::new();
    let task = function(
        &rt,
        "function body() { Promise.resolve().then(() => { try { throw 3; } finally { __rt.effect(0, 1); __rt.need(246, 1); __rt.effect(0, 2); } }).catch(x => __rt.effect(0, x)); return 42; }",
        &[],
    );
    let mut host = Effects::default();
    assert!(matches!(
        task.run_with(Budget::UNLIMITED, &mut host).unwrap(),
        RunOutcome::Waiting(_)
    ));
    assert_eq!(host.0, [Primitive::Number(1.)]);
    rt.collect().unwrap();
    satisfy(&rt, &task);
    assert!(matches!(
        task.run_with(Budget::UNLIMITED, &mut host).unwrap(),
        RunOutcome::Complete(_)
    ));
    assert_eq!(host.0, [1., 2., 3.].map(Primitive::Number));
}
