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

#[test]
fn unhandled_rejections_reported_at_quiescence() {
    // The first body adapts test_async_promise_rejection (license above),
    // replacing its timer with a final reaction job until timers land.
    for (source, reasons) in [
        (
            "let counter=0; let p1=Promise.reject(); let p2=Promise.reject(); let p3=Promise.resolve(); p1.catch(() => counter++); p2.catch(() => counter++); p3.then(() => counter++); Promise.resolve().then(() => { if(counter !== 3) throw counter; }); 42;",
            vec![],
        ),
        ("Promise.reject(1); 42;", vec![1.]),
        (
            "let p=Promise.reject(2); Promise.resolve().then(() => p.catch(() => {})); 42;",
            vec![],
        ),
        ("let p=Promise.reject(3); p.then(x => x); 42;", vec![3.]),
        (
            "async function f() { await 0; throw 4; } f(); 42;",
            vec![4.],
        ),
        (
            "async function f() { throw 5; } (async () => { try { await f(); } catch(e) {} })(); 42;",
            vec![],
        ),
        ("Promise.reject(6).catch(e => { throw e; }); 42;", vec![6.]),
    ] {
        let rt = Runtime::new();
        let task = rt
            .spawn(&compile_script("rejections", source).unwrap(), &[])
            .unwrap();
        // Budget boundaries are not reporting boundaries. Collect around
        // every instruction to exercise the new task roots while running.
        let mut complete = false;
        for _ in 0..5000 {
            match task
                .run_with(Budget::instructions(1), &mut Effects::default())
                .unwrap()
            {
                RunOutcome::Yielded => {
                    assert!(task.unhandled_rejections().entries.is_empty());
                    rt.collect().unwrap();
                }
                RunOutcome::Complete(value) => {
                    assert_eq!(value.view(), ValueView::Number(42.));
                    complete = true;
                    break;
                }
                other => panic!("{source}: {other:?}"),
            }
        }
        assert!(complete, "{source}");
        let report = task.unhandled_rejections();
        assert_eq!(report.dropped, 0);
        assert_eq!(
            report
                .entries
                .iter()
                .map(|r| r.reason.view())
                .collect::<Vec<_>>(),
            reasons
                .into_iter()
                .map(ValueView::Number)
                .collect::<Vec<_>>(),
            "{source}"
        );
    }
    let rt = Runtime::new();
    let task = rt
        .spawn(
            &compile_script(
                "derived",
                "let p=Promise.reject(1); let q=p.then(x => x); __rt.vector(p,q);",
            )
            .unwrap(),
            &[],
        )
        .unwrap();
    let pair = finish(&task);
    let derived = finish(&function(
        &rt,
        "function get(pair) { return __rt.nth(pair,1); }",
        &[pair],
    ));
    assert!(
        task.unhandled_rejections().entries[0]
            .promise
            .same_object(&derived)
    );
}

#[test]
fn rejection_queue_is_bounded_and_later_handling_updates_overflow() {
    let rt = Runtime::new();
    let task = function(
        &rt,
        "function body() { let ps=[]; for(let i=0;i<35;i++) ps.push(Promise.reject(i)); Promise.resolve().then(() => { ps[0].catch(() => {}); ps[34].catch(() => {}); Promise.reject(35); }); return 42; }",
        &[],
    );
    finish(&task);
    let report = task.unhandled_rejections();
    assert_eq!(report.entries.len(), 30);
    assert_eq!(report.dropped, 4);
    assert_eq!(report.entries[0].reason.view(), ValueView::Number(1.));
    assert_eq!(report.entries[29].reason.view(), ValueView::Number(35.));
    let owner = function(
        &rt,
        "function body() { let p; for(let i=0;i<31;i++) p=Promise.reject(i); return p; }",
        &[],
    );
    let omitted = finish(&owner);
    assert_eq!(owner.unhandled_rejections().dropped, 1);
    finish(&function(
        &rt,
        "function handle(p) { p.catch(() => {}); }",
        &[omitted],
    ));
    assert_eq!(owner.unhandled_rejections().dropped, 0);
    assert_eq!(owner.unhandled_rejections().entries.len(), 30);
    // Overflow promises/reasons are not retained, even if they formed cycles.
    let object = rt.scope(|s| s.persist(s.new_object()));
    let weak = object.downgrade();
    let overflow = function(
        &rt,
        "function body(o) { for(let i=0;i<30;i++) Promise.reject(i); Promise.reject(o); }",
        &[object],
    );
    finish(&overflow);
    rt.collect().unwrap();
    assert!(!weak.is_live());
    assert_eq!(overflow.unhandled_rejections().dropped, 1);
}

#[test]
fn unhandled_rejection_roots_are_retained_and_reclaimed() {
    let rt = Runtime::new();
    let object = rt.scope(|s| s.persist(s.new_object()));
    let weak = object.downgrade();
    let task = function(
        &rt,
        "function body(o) { Promise.reject(o); return 42; }",
        &[object],
    );
    finish(&task);
    rt.collect().unwrap();
    assert!(weak.is_live());
    let report = task.unhandled_rejections();
    let promise = report.entries[0].promise.downgrade();
    drop(task);
    rt.collect().unwrap();
    assert!(
        weak.is_live() && promise.is_live(),
        "snapshots own persistent roots"
    );
    drop(report);
    rt.collect().unwrap();
    assert!(!weak.is_live() && !promise.is_live());
}

#[test]
fn foreign_handling_removes_the_rejecting_tasks_record() {
    let rt = Runtime::new();
    let maker = function(
        &rt,
        "function make() { let reject; let p=new Promise((r,j) => { reject=j; }); return __rt.vector(p,reject); }",
        &[],
    );
    let pair = finish(&maker);
    let get = |i| {
        finish(&function(
            &rt,
            &format!("function get(pair) {{ return __rt.nth(pair,{i}); }}"),
            std::slice::from_ref(&pair),
        ))
    };
    let p = get(0);
    let reject = get(1);
    let settler = function(&rt, "function settle(reject) { reject(9); }", &[reject]);
    finish(&settler);
    assert!(maker.unhandled_rejections().entries.is_empty());
    assert!(
        settler.unhandled_rejections().entries[0]
            .promise
            .same_object(&p)
    );
    let handler = function(&rt, "function handle(p) { p.catch(() => {}); }", &[p]);
    finish(&handler);
    assert!(settler.unhandled_rejections().entries.is_empty());
    assert!(handler.unhandled_rejections().entries.is_empty());
}

#[test]
fn rejection_reports_wait_for_dependency_resume_and_discard_on_abort() {
    for abort in ["", "throw 7;", "__rt.need(249,2);"] {
        let rt = Runtime::new();
        let object = rt.scope(|s| s.persist(s.new_object()));
        let weak = object.downgrade();
        let source = format!(
            "function body(o) {{ let p=Promise.reject(o); __rt.need(249,1); {abort} p.catch(() => {{}}); return 42; }}"
        );
        let task = function(&rt, &source, &[object]);
        assert!(matches!(task.run().unwrap(), RunOutcome::Waiting(_)));
        assert!(task.unhandled_rejections().entries.is_empty());
        rt.collect().unwrap();
        assert!(weak.is_live());
        satisfy(&rt, &task);
        match task.run().unwrap() {
            RunOutcome::Complete(_) => assert!(abort.is_empty()),
            RunOutcome::Failed(_) => assert_eq!(abort, "throw 7;"),
            RunOutcome::Waiting(_) => {
                assert!(matches!(task.cancel(), clj_quickjs::Cancel::Runnable(_)));
                assert!(matches!(task.run().unwrap(), RunOutcome::Cancelled));
            }
            other => panic!("{other:?}"),
        }
        assert!(task.unhandled_rejections().entries.is_empty());
        rt.collect().unwrap();
        assert!(!weak.is_live(), "{abort}");
    }
}

#[test]
fn binary_reports_unhandled_rejections_and_exits_nonzero() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join(format!("rejection-cli-{}.js", std::process::id()));
    for (source, status, expected) in [
        (
            "Promise.reject(1); 42;",
            1,
            "Possibly unhandled promise rejection: 1",
        ),
        ("Promise.reject(1).catch(() => {}); 42;", 0, ""),
        (
            "async function f() { await 0; throw 7; } f(); 42;",
            1,
            "Possibly unhandled promise rejection: 7",
        ),
        (
            "for(let i=0;i<32;i++) Promise.reject(i); 42;",
            1,
            "2 additional unhandled promise rejections omitted",
        ),
    ] {
        std::fs::write(&path, source).unwrap();
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_clj-quickjs"))
            .arg(&path)
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(status));
        let stderr = String::from_utf8(out.stderr).unwrap();
        if expected.is_empty() {
            assert!(stderr.is_empty(), "{stderr}");
        } else {
            assert!(stderr.contains(expected), "{stderr}");
        }
    }
    std::fs::remove_file(path).unwrap();
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
fn thenable_then_waits_and_resumes() {
    let rt = Runtime::new();
    let task = function(
        &rt,
        r#"function body() {
        let calls = 0;
        Promise.resolve({then(resolve, reject) {
            __rt.effect(0, ++calls);
            resolve(__rt.need(247, 1)); resolve(99); reject(98); throw 97;
        }}).then(x => __rt.effect(0, x));
        Promise.resolve().then(() => __rt.effect(0, 2));
        __rt.effect(0, 0); return 42;
    }"#,
        &[],
    );
    let mut host = Effects::default();
    assert!(matches!(
        task.run_with(Budget::UNLIMITED, &mut host).unwrap(),
        RunOutcome::Waiting(_)
    ));
    assert_eq!(host.0, [0., 1.].map(Primitive::Number));
    rt.collect().unwrap();
    satisfy(&rt, &task);
    let (tx, rx) = std::sync::mpsc::channel();
    let resumed = task.clone();
    let thread = std::thread::spawn(move || {
        let outcome = resumed.run_with(Budget::UNLIMITED, &mut host).unwrap();
        tx.send((outcome, host)).unwrap();
    });
    let (outcome, host) = rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap();
    thread.join().unwrap();
    let RunOutcome::Complete(value) = outcome else {
        panic!("{outcome:?}")
    };
    assert_eq!(value.view(), ValueView::Number(42.));
    assert_eq!(host.0, [0., 1., 2., 8.].map(Primitive::Number));
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
        resolve(p); p.catch(e => __rt.effect(0, e instanceof TypeError && typeof e.stack === "string"));
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
fn missing_fulfillment_handler_resolves_the_derived_promise() {
    for handlers in ["", "null", "42, false"] {
        let (result, effects) = run(&format!(
            r#"
                let reads = 0;
                const value = {{get then() {{ if (++reads === 2) throw 7; }}}};
                Promise.resolve(value).then({handlers}).catch(reason => {{
                    __rt.effect(0, reason);
                    __rt.effect(0, reads);
                }});
                __rt.effect(0, reads);
                42;
            "#
        ));
        assert_eq!(result, ValueView::Number(42.));
        assert_eq!(effects, [1., 7., 2.].map(Primitive::Number));
    }

    let (_, effects) = run(r#"
        let reads = 0;
        const value = {get then() { reads++; return 3; }};
        Promise.resolve(value).then().then(result => {
            __rt.effect(0, result === value);
            __rt.effect(0, reads);
        });
    "#);
    assert_eq!(effects, [Primitive::Bool(true), Primitive::Number(2.)]);
}

#[test]
fn missing_rejection_handler_preserves_thenable_reason() {
    for handlers in ["", "null, 7"] {
        for value in [
            "{then(resolve) { __rt.effect(0, 99); resolve(1); }}",
            "{get then() { __rt.effect(0, 99); throw 7; }}",
        ] {
            let (_, effects) = run(&format!(
                "const reason = {value}; Promise.reject(reason).then({handlers}).catch(value => __rt.effect(0, value === reason));"
            ));
            assert_eq!(effects, [Primitive::Bool(true)]);
        }
    }
}

#[test]
fn missing_fulfillment_handler_then_getter_waits_without_replay() {
    let rt = Runtime::new();
    let task = function(
        &rt,
        r#"function body() {
            let reads = 0;
            const value = {get then() {
                if (++reads === 2) {
                    __rt.effect(0, 1);
                    __rt.need(246, 1);
                    __rt.effect(0, reads);
                    throw 7;
                }
            }};
            Promise.resolve(value).then().catch(reason => __rt.effect(0, reason));
            return 42;
        }"#,
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
    let (tx, rx) = std::sync::mpsc::channel();
    let thread = std::thread::spawn(move || {
        let out = task.run_with(Budget::UNLIMITED, &mut host).unwrap();
        tx.send((out, host)).unwrap();
    });
    let (out, host) = rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap();
    thread.join().unwrap();
    let RunOutcome::Complete(value) = out else {
        panic!("{out:?}")
    };
    assert_eq!(value.view(), ValueView::Number(42.));
    assert_eq!(host.0, [1., 2., 7.].map(Primitive::Number));
}

#[test]
fn thenable_adoption_replaces_explicit_gap() {
    for (source, expected) in [
        (
            "Promise.resolve({then(resolve) { resolve(1); }}).then(x => __rt.effect(0, x));",
            vec![1.],
        ),
        (
            "new Promise(resolve => resolve({get then() { return function() { __rt.effect(0, 3); }; }}));",
            vec![3.],
        ),
        (
            "Promise.resolve(1).then(() => Promise.resolve(2)).then(x => __rt.effect(0, x));",
            vec![2.],
        ),
        (
            "let reads = 0; Promise.resolve({get then() { if (++reads === 2) return function() { __rt.effect(0, 99); }; }}).then();",
            vec![99.],
        ),
    ] {
        let (_, effects) = run(source);
        assert_eq!(
            effects,
            expected
                .into_iter()
                .map(Primitive::Number)
                .collect::<Vec<_>>()
        );
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
        "function consume(p) { p.then(x => __rt.effect(0, 'bad')).catch(e => __rt.effect(0, e instanceof TypeError && typeof e.stack === 'string')); return 42; }",
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
fn thenable_resolving_functions_are_traced() {
    let rt = Runtime::new();
    let object = rt.scope(|scope| scope.persist(scope.new_object()));
    let weak = object.downgrade();
    let maker = function(
        &rt,
        r#"function make(o) {
        let saved;
        Promise.resolve({then(resolve) { saved = resolve; resolve(o); }});
        return () => saved;
    }"#,
        &[object],
    );
    let holder = finish(&maker);
    drop(maker);
    rt.collect().unwrap();
    assert!(weak.is_live());
    assert_eq!(rt.heap_usage().objects.promise_resolving, 1);
    drop(holder);
    rt.collect().unwrap();
    assert!(!weak.is_live());
    assert_eq!(rt.heap_usage().objects.promise_resolving, 0);
    assert_eq!(rt.heap_usage().objects.promise, 0);
}

#[test]
fn adoption_of_foreign_task_local_result_rejects() {
    for method in ["resolve", "reject"] {
        let rt = Runtime::new();
        let inner = finish(&function(
            &rt,
            &format!("function make() {{ return Promise.{method}({{value: 1}}); }}"),
            &[],
        ));
        rt.publish(&inner).unwrap();
        let task = function(
            &rt,
            r#"function adopt(inner) {
            new Promise(r => r(inner))
                .then(() => __rt.effect(0, 99))
                .catch(e => __rt.effect(0, e instanceof TypeError));
            // Ordinary handlers still cannot receive a foreign result.
            inner.then(() => __rt.effect(0, 98), () => __rt.effect(0, 97))
                .catch(e => __rt.effect(0, e instanceof TypeError));
            return 42;
        }"#,
            std::slice::from_ref(&inner),
        );
        let mut host = Effects::default();
        let outcome = task.run_with(Budget::UNLIMITED, &mut host).unwrap();
        assert!(matches!(outcome, RunOutcome::Complete(_)), "{outcome:?}");
        assert_eq!(host.0, [Primitive::Bool(true), Primitive::Bool(true)]);
        assert!(task.unhandled_rejections().entries.is_empty());
        assert_eq!(task.unhandled_rejections().dropped, 0);
        let async_task = function(
            &rt,
            r#"function consume(inner) {
            async function f() { return new Promise(r => r(inner)); }
            (async () => { try { await f(); } catch(e) { __rt.effect(0,e instanceof TypeError); } })();
            return 42;
        }"#,
            &[inner],
        );
        let mut host = Effects::default();
        assert!(matches!(
            async_task.run_with(Budget::UNLIMITED, &mut host).unwrap(),
            RunOutcome::Complete(_)
        ));
        assert_eq!(host.0, [Primitive::Bool(true)]);
        assert!(async_task.unhandled_rejections().entries.is_empty());
        assert_eq!(async_task.unhandled_rejections().dropped, 0);
    }
}

#[test]
fn thenable_resolvers_settle_after_then_returns() {
    for reject_first in [false, true] {
        let rt = Runtime::new();
        let task = function(
            &rt,
            &format!(
                r#"function body() {{
            let resolve; let reject; let settled = 0;
            Promise.resolve({{then(r, j) {{ resolve = r; reject = j; return 99; }}}})
                .then(v => {{ settled++; __rt.effect(0, v); }},
                      e => {{ settled++; __rt.effect(0, -e); }});
            Promise.resolve().then(() => {{
                __rt.effect(0, settled);
                if ({reject_first}) {{ reject(7); resolve(8); }}
                else {{ resolve(7); reject(8); }}
                resolve(9); reject(10);
            }}).then(() => __rt.effect(0, settled));
            return 42;
        }}"#
            ),
            &[],
        );
        let mut host = Effects::default();
        for _ in 0..2000 {
            match task.run_with(Budget::instructions(1), &mut host).unwrap() {
                RunOutcome::Yielded => {
                    rt.collect().unwrap();
                }
                RunOutcome::Complete(value) => {
                    assert_eq!(value.view(), ValueView::Number(42.));
                    assert_eq!(
                        host.0,
                        [0., if reject_first { -7. } else { 7. }, 1.].map(Primitive::Number)
                    );
                    break;
                }
                outcome => panic!("{outcome:?}"),
            }
        }
        assert_eq!(task.status(), clj_quickjs::TaskStatus::Finished);
    }
}

#[test]
fn thenable_jobs_preserve_saved_then_across_budgeting_and_collection() {
    let rt = Runtime::new();
    let task = function(
        &rt,
        r#"function body() {
        let calls = 0;
        const value = {then(resolve, reject) {
            __rt.effect(0, ++calls);
            resolve({then(r) { r(8); }}); reject(99); throw 98;
        }};
        Promise.resolve(value).then(x => __rt.effect(0, x));
        value.then = () => __rt.effect(0, 97);
        return 42;
    }"#,
        &[],
    );
    let mut host = Effects::default();
    for _ in 0..2000 {
        match task.run_with(Budget::instructions(1), &mut host).unwrap() {
            RunOutcome::Yielded => {
                rt.collect().unwrap();
            }
            RunOutcome::Complete(value) => {
                assert_eq!(value.view(), ValueView::Number(42.));
                assert_eq!(host.0, [1., 8.].map(Primitive::Number));
                return;
            }
            outcome => panic!("{outcome:?}"),
        }
    }
    panic!("thenable execution did not complete");
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

#[test]
fn promise_objects_expose_their_intrinsic_prototype() {
    assert_eq!(
        run(r#"
            const p = Promise.resolve();
            Object.getPrototypeOf(p) === Promise.prototype &&
            p instanceof Promise && p instanceof Object &&
            Object.prototype.toString.call(p) === '[object Promise]';
        "#)
        .0,
        ValueView::Bool(true),
    );
}

#[test]
fn promise_inherited_setters_follow_the_prototype_chain() {
    for prototype in ["p", "Object.create(p)"] {
        for assignment in ["o.__proto__ = target", "o[key] = target"] {
            assert_eq!(
                run(&format!(
                    r#"
                        const p = Promise.resolve();
                        const o = Object.create({prototype});
                        const target = {{}};
                        const key = '__proto__';
                        {assignment};
                        Object.getPrototypeOf(o) === target &&
                        Object.getOwnPropertyDescriptor(o, '__proto__') === undefined &&
                        Object.getPrototypeOf(p) === Promise.prototype;
                    "#
                ))
                .0,
                ValueView::Bool(true),
                "prototype: {prototype}, assignment: {assignment}",
            );
        }
    }
}

#[test]
fn promise_executor_rejects_first_and_ignores_later_completions() {
    let (result, effects) = run(r#"
        new Promise((resolve, reject) => {
            reject(7); reject(8); resolve(9); throw 10;
        }).then(
            value => __rt.effect(0, 'fulfilled'),
            reason => __rt.effect(0, reason)
        );
        __rt.effect(0, 0);
        42;
    "#);
    assert_eq!(result, ValueView::Number(42.));
    assert_eq!(effects, [0., 7.].map(Primitive::Number));
}

#[test]
fn promise_runtime_errors_reject() {
    for expression in [
        "new Promise(() => null.x)",
        "Promise.resolve().then(() => null.x)",
        "Promise.resolve({get then() { return null.x; }})",
    ] {
        let (result, effects) = run(&format!(
            r#"
                let caught = 0;
                {expression}.then(() => __rt.effect(0, 'fulfilled')).catch(error => {{
                    __rt.effect(0, error instanceof TypeError);
                    caught++;
                }}).then(() => __rt.effect(0, caught));
                __rt.effect(0, 'body');
                42;
            "#
        ));
        assert_eq!(result, ValueView::Number(42.), "{expression}");
        assert_eq!(
            effects,
            [
                Primitive::String("body".into()),
                Primitive::Bool(true),
                Primitive::Number(1.),
            ],
            "{expression}",
        );
    }
}

#[test]
fn promise_dependency_failure_rejects_after_resume() {
    let rt = Runtime::new();
    let task = function(
        &rt,
        r#"function body() {
            Promise.resolve().then(() => {
                __rt.effect(0, 1);
                __rt.need(246, 1);
                __rt.effect(0, 'unreachable');
            }).catch(error => {
                __rt.effect(0, error instanceof Error && error.cause === 7);
            }).then(() => __rt.effect(0, 4));
            Promise.resolve().then(() => __rt.effect(0, 2));
            __rt.effect(0, 0);
            return 42;
        }"#,
        &[],
    );
    let mut host = Effects::default();
    assert!(matches!(
        task.run_with(Budget::UNLIMITED, &mut host).unwrap(),
        RunOutcome::Waiting(_)
    ));
    assert_eq!(host.0, [0., 1.].map(Primitive::Number));
    rt.collect().unwrap();
    let request = task.take_producer_requests().pop().unwrap();
    let failure = rt.root(Primitive::Number(7.));
    let wakes = rt
        .complete(
            request.key(),
            request.producer(),
            clj_quickjs::Completion::Failed(&failure),
        )
        .unwrap();
    assert_eq!(wakes.task_ids(), [task.id()]);
    let (tx, rx) = std::sync::mpsc::channel();
    let resumed = task.clone();
    let thread = std::thread::spawn(move || {
        let outcome = resumed.run_with(Budget::UNLIMITED, &mut host).unwrap();
        tx.send((outcome, host)).unwrap();
    });
    let (outcome, host) = rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap();
    thread.join().unwrap();
    let RunOutcome::Complete(value) = outcome else {
        panic!("{outcome:?}")
    };
    assert_eq!(value.view(), ValueView::Number(42.));
    assert_eq!(
        host.0,
        [
            Primitive::Number(0.),
            Primitive::Number(1.),
            Primitive::Number(2.),
            Primitive::Bool(true),
            Primitive::Number(4.),
        ],
    );
    assert_eq!(task.status(), clj_quickjs::TaskStatus::Finished);
}

#[test]
fn async_function_needs_and_awaits() {
    let rt = Runtime::new();
    let task = function(
        &rt,
        r#"function body() {
        async function f() {
            __rt.effect(0, 'first');
            let a = __rt.need(248,1);
            __rt.effect(0, 'before-await');
            await null;
            __rt.effect(0, 'second');
            let b = __rt.need(248,2);
            return a + b;
        }
        f().then(x => __rt.effect(0,x));
        __rt.effect(0, 'caller');
        Promise.resolve().then(() => __rt.effect(0,'parallel'));
        return 42;
    }"#,
        &[],
    );
    let mut host = Effects::default();
    assert!(matches!(
        task.run_with(Budget::UNLIMITED, &mut host).unwrap(),
        RunOutcome::Waiting(_)
    ));
    assert_eq!(host.0, [Primitive::String("first".into())]);
    rt.collect().unwrap();
    satisfy(&rt, &task);
    let moved = task.clone();
    let (out, returned_host) = std::thread::spawn(move || {
        let out = moved.run_with(Budget::UNLIMITED, &mut host).unwrap();
        (out, host)
    })
    .join()
    .unwrap();
    host = returned_host;
    assert!(matches!(out, RunOutcome::Waiting(_)));
    assert_eq!(
        host.0,
        ["first", "before-await", "caller", "second"].map(|s| Primitive::String(s.into()))
    );
    rt.collect().unwrap();
    satisfy(&rt, &task);
    let moved = task.clone();
    let (out, host) = std::thread::spawn(move || {
        let out = moved.run_with(Budget::UNLIMITED, &mut host).unwrap();
        (out, host)
    })
    .join()
    .unwrap();
    assert!(matches!(out,RunOutcome::Complete(v) if v.view()==ValueView::Number(42.)));
    assert_eq!(
        host.0,
        vec![
            Primitive::String("first".into()),
            Primitive::String("before-await".into()),
            Primitive::String("caller".into()),
            Primitive::String("second".into()),
            Primitive::String("parallel".into()),
            Primitive::Number(16.)
        ]
    );
    assert!(task.suspended_async_activations().is_empty());
}

#[test]
fn async_await_and_return_thenables_are_resumable() {
    let rt = Runtime::new();
    let task = function(
        &rt,
        r#"function body() {
        async function f() {
            let x = await {get then() { __rt.need(248,3); return resolve => resolve(5); }};
            return {then(resolve) { __rt.need(248,4); resolve(x + 1); }};
        }
        f().then(x => __rt.effect(0,x));
        __rt.effect(0,1);
        return 42;
    }"#,
        &[],
    );
    let mut host = Effects::default();
    assert!(matches!(
        task.run_with(Budget::UNLIMITED, &mut host).unwrap(),
        RunOutcome::Waiting(_)
    ));
    assert!(
        host.0.is_empty(),
        "then getter is synchronous before suspension"
    );
    rt.collect().unwrap();
    satisfy(&rt, &task);
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
    assert_eq!(host.0, [Primitive::Number(1.), Primitive::Number(6.)]);
}

#[test]
fn async_resume_jobs_belong_to_the_awaiting_task() {
    for result in ["2", "({private:2})"] {
        let rt = Runtime::new();
        let pair = finish(&function(
            &rt,
            "function make() { let resolve; let p=new Promise(r => { resolve=r; }); return __rt.vector(p,resolve); }",
            &[],
        ));
        rt.publish(&pair).unwrap();
        let get = |i| {
            finish(&function(
                &rt,
                &format!("function get(pair) {{ return __rt.nth(pair,{i}); }}"),
                std::slice::from_ref(&pair),
            ))
        };
        let p = get(0);
        let resolve = get(1);
        let consumer = function(
            &rt,
            r#"function consume(p) {
            (async () => { try { return await p; } catch(e) { return e instanceof TypeError ? 3 : 99; } })().then(x => __rt.effect(0,x));
            __rt.need(248,5);
            return 42;
        }"#,
            &[p],
        );
        let mut host = Effects::default();
        assert!(matches!(
            consumer.run_with(Budget::UNLIMITED, &mut host).unwrap(),
            RunOutcome::Waiting(_)
        ));
        let settler = function(
            &rt,
            &format!("function settle(resolve) {{ resolve({result}); }}"),
            &[resolve],
        );
        finish(&settler);
        assert!(host.0.is_empty());
        assert_eq!(consumer.status(), clj_quickjs::TaskStatus::Waiting);
        rt.collect().unwrap();
        satisfy(&rt, &consumer);
        assert!(matches!(
            consumer.run_with(Budget::UNLIMITED, &mut host).unwrap(),
            RunOutcome::Complete(_)
        ));
        assert_eq!(
            host.0,
            [Primitive::Number(if result == "2" { 2. } else { 3. })]
        );
        assert!(consumer.suspended_async_activations().is_empty());
    }
}

#[test]
fn async_operands_and_handlers_survive_collection_at_every_step() {
    for body in [
        "function first(a,b) { return a; } return first({n:17}, await 0).n;",
        "try { return {n:17}; } finally { await 0; await null; }",
        "try { throw {n:17}; } finally { await 0; }",
        "try { return 2 + await Promise.reject({n:17}); } catch(e) { return e; }",
        "let {[await 'x']: value = await 17} = {}; return value;",
        "let o = {[await 'x']: await 17}; return o.x;",
        "let o = {[await 'm']() { return this.v; }, v: 17}; return o.m();",
    ] {
        let rt = Runtime::new();
        let source = format!(
            "function body() {{ async function f() {{ {body} }} f().then(x => __rt.effect(0,typeof x === 'number' ? x : x.n), e => __rt.effect(0,e.n)); return 42; }}"
        );
        let task = function(&rt, &source, &[]);
        let mut host = Effects::default();
        let mut complete = false;
        for _ in 0..5000 {
            match task.run_with(Budget::instructions(1), &mut host).unwrap() {
                RunOutcome::Yielded => {
                    rt.collect().unwrap();
                }
                RunOutcome::Complete(v) => {
                    assert_eq!(v.view(), ValueView::Number(42.));
                    complete = true;
                    break;
                }
                other => panic!("{body}: {other:?}"),
            }
        }
        assert!(complete, "{body}");
        assert_eq!(host.0, [Primitive::Number(17.)], "{body}");
        rt.collect().unwrap();
        assert_eq!(rt.heap_usage().objects.async_state, 0);
    }
}

#[test]
fn async_cancellation_unwinds_attached_finally_and_discards_resumes() {
    let rt = Runtime::new();
    let object = rt.scope(|s| s.persist(s.new_object()));
    let weak = object.downgrade();
    let task = function(
        &rt,
        r#"function body(o) {
        (async () => { await 0; try { __rt.need(248,6); } finally { __rt.effect(0,1); } })();
        (async () => { o; await 0; __rt.effect(0,2); })();
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
    assert_eq!(host.0, [Primitive::Number(1.)]);
    rt.collect().unwrap();
    assert!(!weak.is_live());
    assert_eq!(rt.heap_usage().objects.async_state, 0);
}

#[test]
fn async_settlement_overflow_rejects_instead_of_stalling() {
    let rt = Runtime::with_limits(clj_quickjs::VmLimits {
        max_call_depth: 2,
        ..clj_quickjs::VmLimits::DEFAULT
    });
    let task=rt.spawn(&compile_script("async-overflow", "Promise.resolve().then(async () => 1).then(x => __rt.effect(0,x), e => __rt.effect(0,e instanceof Error ? 99 : 98)); 42;").unwrap(),&[]).unwrap();
    let mut host = Effects::default();
    let out = task.run_with(Budget::UNLIMITED, &mut host).unwrap();
    assert!(matches!(out,RunOutcome::Complete(v) if v.view()==ValueView::Number(42.)));
    assert_eq!(host.0, [Primitive::Number(99.)]);
    assert!(task.suspended_async_activations().is_empty());
}
