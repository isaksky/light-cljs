//! The reserved `__rt.need` intrinsic lowered to the resumable VM `need`
//! instruction (pellet clj-quickjs-23, `docs/design.md` §§7, 10), through
//! the public embedding API. Programs are JavaScript source: the synthetic
//! fixtures `fixtures/js/need_consumer.js` and `need_producer.js` (not
//! light-cljs output) and inline snippets.
//!
//! The tests play a deterministic, manually driven host: they run tasks on
//! threads they own, take the producer requests a suspended task's needs
//! claimed, run producer tasks, publish results, and enqueue (here: run) the
//! tasks the completion's wakeups return. No promise, callback or retained
//! Rust frame stands for a waiting task: a suspended task is plain VM state,
//! and the run that resumes it may be on another thread.

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use clj_quickjs::exec::{
    EnqueueReason, Event, EventKind, Executor, ExecutorConfig, OutcomeKind, Placement, WorkerId,
};
use clj_quickjs::frontend::{SyntaxErrorKind, compile_function, compile_script};
use clj_quickjs::{
    Budget, Cancel, CellStatus, Completion, DependencyId, DependencyKey, Executable, Host,
    HostCall, Primitive, ProducerRequest, ResultKind, RunOutcome, Runtime, Task, TaskStatus,
    ValueView, VmError, VmErrorKind, Wakeups,
};

/// `__rt.need(5, 3)`: a number.
const NUMBER: (DependencyId, ResultKind) = (DependencyId::new(5), ResultKind::new(3));
/// `__rt.need(6, 2)`: a callable.
const CALLABLE: (DependencyId, ResultKind) = (DependencyId::new(6), ResultKind::new(2));

/// Synthetic fixtures, not light-cljs output. Park inside key conversion or
/// value evaluation, collect every yield, then migrate to another worker.
#[test]
fn computed_property_names_wait_and_migrate() {
    for body in [
        "let o = {[k]: (__rt.effect(1, 2), 7)}; o.m + n",
        "let o = {[k]() { return this.v; }, v: 7}; o.m() + n",
        "let o = {get [k]() { return this.v; }, v: 7}; o.m + n",
        "let o = {set [k](v) { this.v = v; }}; o.m = 7; o.v + n",
        "let s = Symbol('s'); let o = {[s]: (__rt.need(5, 3), 7)}; o[s] + n",
    ] {
        let rt = Runtime::new();
        let source = format!(
            "let n = 0; let k = {{toString() {{ n++; __rt.effect(1, 1); __rt.need(5, 3); return 'm'; }} }}; {body}"
        );
        let task = rt
            .spawn(&compile_script("computed-wait", &source).unwrap(), &[])
            .unwrap();
        let mut host = Recorder::default();
        let expected = key(&rt, NUMBER);
        let mut parked = false;
        for _ in 0..2000 {
            match task.run_with(Budget::instructions(1), &mut host).unwrap() {
                RunOutcome::Yielded => {
                    rt.collect().unwrap();
                }
                outcome => {
                    assert_eq!(waiting(outcome), expected);
                    parked = true;
                    break;
                }
            }
        }
        assert!(parked, "bounded wait: {source}");
        let request = one_request(&task, expected);
        rt.collect().unwrap();
        let resumed = only_runnable(publish_number(&rt, &request, 42.0));
        let symbol_value = body.contains("Symbol");
        let (result, host) = thread::spawn(move || {
            for _ in 0..2000 {
                match resumed
                    .run_with(Budget::instructions(1), &mut host)
                    .unwrap()
                {
                    RunOutcome::Yielded => {
                        rt.collect().unwrap();
                    }
                    outcome => return (number(outcome), host),
                }
            }
            panic!("bounded completion")
        })
        .join()
        .unwrap();
        assert_eq!(result, if symbol_value { 7.0 } else { 8.0 });
        let expected_effects: &[f64] = if symbol_value {
            &[]
        } else if body.contains("effect(1, 2)") {
            &[1.0, 2.0]
        } else {
            &[1.0]
        };
        assert_eq!(host.effects, expected_effects, "effects run once: {source}");
    }
}

#[test]
fn iterator_next_waits_on_dependency() {
    // Synthetic fixtures, not light-cljs output. Collect at every yield and
    // move the suspended task to another worker after publication.
    for expression in [
        "[...it]",
        "(function(a, b) { return [a, b]; })(...it)",
        "({n: true, m(a, b) { return this.n ? [a, b] : []; }}).m(...it)",
        "new (function(a, b) { this.a = a; return [this.a, b]; })(...it)",
    ] {
        let rt = Runtime::new();
        let source = format!(
            "let n = 0; let it = {{}}; it[Symbol.iterator] = function() {{ return {{next() {{ n++; __rt.effect(1, n); let v = __rt.need(5, 3); return {{done: n > 2, value: {{n: v + n}}}}; }} }}; }}; let x = {expression}; x[0].n + x[1].n + n"
        );
        let task = rt
            .spawn(&compile_script("spread-wait", &source).unwrap(), &[])
            .unwrap();
        let mut host = Recorder::default();
        let expected = key(&rt, NUMBER);
        for steps in 0..2000 {
            match task.run_with(Budget::instructions(1), &mut host).unwrap() {
                RunOutcome::Yielded => {
                    rt.collect().unwrap();
                }
                outcome => {
                    assert_eq!(waiting(outcome), expected);
                    break;
                }
            }
            assert!(steps < 1999);
        }
        assert_eq!(host.effects, [1.0]);
        let request = one_request(&task, expected);
        rt.collect().unwrap();
        let resumed = only_runnable(publish_number(&rt, &request, 7.0));
        let (result, host) = thread::spawn(move || {
            for steps in 0..2000 {
                match resumed
                    .run_with(Budget::instructions(1), &mut host)
                    .unwrap()
                {
                    RunOutcome::Yielded => {
                        rt.collect().unwrap();
                    }
                    outcome => return (number(outcome), host),
                }
                assert!(steps < 1999);
            }
            unreachable!()
        })
        .join()
        .unwrap();
        assert_eq!(result, 20.0);
        assert_eq!(host.effects, [1.0, 2.0, 3.0]);
    }
}

#[test]
fn spread_builtin_calls_wait_and_resume() {
    // Synthetic fixtures, not light-cljs output. The wait occurs after
    // packed argument expansion, inside the built-in's native continuation.
    for source in [
        "let obj = {valueOf() { __rt.effect(1, 1); return __rt.need(5, 3); }}; Number(...[obj])",
        "let obj = {valueOf() { __rt.effect(1, 1); return __rt.need(5, 3); }}; new Number(...[obj]).valueOf()",
        "let receiver = {get length() { __rt.effect(1, 1); return __rt.need(5, 3); }, set length(n) { this.n = n; }}; Array.prototype.push.call(...[receiver, {n: 7}]); receiver[7].n + receiver.n - 8",
    ] {
        let rt = Runtime::new();
        let task = rt
            .spawn(&compile_script("spread-builtin-wait", source).unwrap(), &[])
            .unwrap();
        let expected = key(&rt, NUMBER);
        let mut host = Recorder::default();
        for steps in 0..2000 {
            match task.run_with(Budget::instructions(1), &mut host).unwrap() {
                RunOutcome::Yielded => {
                    rt.collect().unwrap();
                }
                outcome => {
                    assert_eq!(waiting(outcome), expected);
                    break;
                }
            }
            assert!(steps < 1999);
        }
        assert_eq!(host.effects, [1.0]);
        let request = one_request(&task, expected);
        rt.collect().unwrap();
        let resumed = only_runnable(publish_number(&rt, &request, 7.0));
        let (result, host) = thread::spawn(move || {
            for steps in 0..2000 {
                match resumed
                    .run_with(Budget::instructions(1), &mut host)
                    .unwrap()
                {
                    RunOutcome::Yielded => {
                        rt.collect().unwrap();
                    }
                    outcome => return (number(outcome), host),
                }
                assert!(steps < 1999);
            }
            unreachable!()
        })
        .join()
        .unwrap();
        assert_eq!(result, 7.0, "{source}");
        assert_eq!(host.effects, [1.0], "callbacks must not replay: {source}");
    }
}

#[test]
fn generator_body_waits_and_resumes() {
    // Synthetic fixture, not light-cljs output. `g` is retained by a local
    // while its attached activation waits; collection also sees detached locals.
    let rt = Runtime::new();
    let exe = compile_script(
        "generator-wait",
        r#"
        function* f() {
            let held = {n: 2};
            yield held.n;
            let n = __rt.need(5, 3);
            yield n + held.n;
            return 11;
        }
        let g = f();
        let a = g.next();
        let b = g.next();
        let c = g.next();
        if (a.done || b.done || !c.done) throw 'done';
        a.value + b.value + c.value
    "#,
    )
    .unwrap();
    let task = rt.spawn(&exe, &[]).unwrap();
    let mut host = Recorder::default();
    let expected = key(&rt, NUMBER);
    for step in 0..1000 {
        match task.run_with(Budget::instructions(1), &mut host).unwrap() {
            RunOutcome::Yielded => {
                rt.collect().unwrap();
            }
            outcome => {
                assert_eq!(waiting(outcome), expected);
                break;
            }
        }
        assert!(step < 999, "never reached need");
    }
    let request = one_request(&task, expected);
    rt.collect().unwrap();
    let resumed = only_runnable(publish_number(&rt, &request, 7.0));
    let value = thread::spawn(move || number(run(&resumed, &mut host)))
        .join()
        .unwrap();
    assert_eq!(value, 22.0);
}

#[test]
fn yield_star_inner_next_waits() {
    // Synthetic fixture, not light-cljs output. Effects detect repeated next
    // calls; collection at every budget yield covers attached/detached state.
    let rt = Runtime::new();
    let exe = compile_script(
        "yield-star-wait",
        r#"
        let n = 0;
        let it = {next(v) {
            n++; __rt.effect(1, n);
            let dependency = __rt.need(5, 3);
            return {done: n > 2, value: dependency + n + (v === undefined ? 0 : v)};
        }};
        it[Symbol.iterator] = function() { return this; };
        function* outer() { return 10 + (yield* it); }
        let g = outer(), a = g.next(), b = g.next(20), c = g.next(30), d = g.next();
        if (a.value !== 8 || a.done || b.value !== 29 || b.done ||
            c.value !== 50 || !c.done || d.value !== undefined || !d.done || n !== 3)
            throw 'delegated sequence';
        c.value
    "#,
    )
    .unwrap();
    let task = rt.spawn(&exe, &[]).unwrap();
    let mut host = Recorder::default();
    let expected = key(&rt, NUMBER);
    for step in 0..2000 {
        match task.run_with(Budget::instructions(1), &mut host).unwrap() {
            RunOutcome::Yielded => {
                rt.collect().unwrap();
            }
            outcome => {
                assert_eq!(waiting(outcome), expected);
                break;
            }
        }
        assert!(step < 1999, "never reached inner need");
    }
    let request = one_request(&task, expected);
    rt.collect().unwrap();
    let resumed = only_runnable(publish_number(&rt, &request, 7.0));
    let value = thread::spawn(move || {
        for step in 0..3000 {
            match resumed
                .run_with(Budget::instructions(1), &mut host)
                .unwrap()
            {
                RunOutcome::Yielded => {
                    rt.collect().unwrap();
                }
                outcome => {
                    assert_eq!(host.effects, [1.0, 2.0, 3.0]);
                    return number(outcome);
                }
            }
            assert!(step < 2999, "delegation did not complete");
        }
        unreachable!()
    })
    .join()
    .unwrap();
    assert_eq!(value, 50.0);
}

#[test]
fn yield_star_protocol_getters_wait() {
    // Synthetic fixtures, not light-cljs output. Isolate a wait in each
    // protocol callback beneath the generator's native exception boundary.
    let fixture = r#"
        let counts = [0, 0, 0, 0, 0, 0], calls = 0, valueReads = 0;
        function pause(stage) {
            counts[stage]++;
            __rt.effect(1, stage);
            let held = {n: 7};
            if (stage === waitingStage && __rt.need(5, 3) !== held.n) throw 'lost local';
            return held.n;
        }
        let fixed = {
            get done() { pause(4); return false; },
            get value() { valueReads++; return 1; }
        };
        let final = {
            get done() { pause(4); return true; },
            get value() { return pause(5) + calls; }
        };
        let it = {
            get next() {
                pause(3);
                return function(received) {
                    __rt.effect(1, 6);
                    calls++;
                    if (this !== it || arguments.length !== 1 ||
                        (calls === 1 && received !== undefined) ||
                        (calls === 2 && received !== 20)) throw 'next invocation';
                    return calls === 1 ? fixed : final;
                };
            }
        };
        Object.defineProperty(it, Symbol.iterator, {get: function() {
            pause(1);
            return function() { pause(2); return this; };
        }});
        function* outer() { return 10 + (yield* it); }
        let g = outer(), a = g.next('ignored');
        if (a !== fixed || valueReads !== 0) throw 'result identity';
        let b = g.next(20);
        if (b.value !== 19 || !b.done || calls !== 2 || counts[1] !== 1 ||
            counts[2] !== 1 || counts[3] !== 1 || counts[4] !== 2 || counts[5] !== 1)
            throw 'protocol replay';
        b.value
    "#;
    for stage in 1..=5 {
        let rt = Runtime::new();
        let source = format!("let waitingStage = {stage};\n{fixture}");
        let exe = compile_script("yield-star-protocol-wait", &source).unwrap();
        let task = rt.spawn(&exe, &[]).unwrap();
        let mut host = Recorder::default();
        let expected = key(&rt, NUMBER);
        for step in 0..3000 {
            match task.run_with(Budget::instructions(1), &mut host).unwrap() {
                RunOutcome::Yielded => {
                    rt.collect().unwrap();
                }
                outcome => {
                    assert_eq!(waiting(outcome), expected, "protocol stage {stage}");
                    break;
                }
            }
            assert!(step < 2999, "stage {stage} never reached need");
        }
        let request = one_request(&task, expected);
        rt.collect().unwrap();
        let resumed = only_runnable(publish_number(&rt, &request, 7.0));
        let (value, host) = thread::spawn(move || {
            for step in 0..3000 {
                match resumed
                    .run_with(Budget::instructions(1), &mut host)
                    .unwrap()
                {
                    RunOutcome::Yielded => {
                        rt.collect().unwrap();
                    }
                    outcome => return (number(outcome), host),
                }
                assert!(step < 2999, "protocol stage {stage} did not complete");
            }
            unreachable!()
        })
        .join()
        .unwrap();
        assert_eq!(value, 19.0, "protocol stage {stage}");
        assert_eq!(host.effects, [1.0, 2.0, 3.0, 6.0, 4.0, 6.0, 4.0, 5.0]);
    }
}

#[test]
fn generator_cancellation_runs_finally() {
    // Synthetic fixture, not light-cljs output.
    let rt = Runtime::new();
    let exe = compile_script("generator-cancel", "function* f() { try { yield 1; __rt.need(5,3); } finally { __rt.effect(2,9); } } let g=f(); g.next(); g.next();").unwrap();
    let task = rt.spawn(&exe, &[]).unwrap();
    let mut host = Recorder::default();
    assert_eq!(waiting(run(&task, &mut host)), key(&rt, NUMBER));
    let Cancel::Runnable(cancelled) = task.cancel() else {
        panic!("runnable cancellation")
    };
    assert!(matches!(run(&cancelled, &mut host), RunOutcome::Cancelled));
    assert_eq!(host.reports, [9.0]);
}

#[test]
fn generator_cancellation_cannot_detach_cleanup() {
    // Synthetic fixture, not light-cljs output. A yield cannot move the
    // pending cancellation into an inert generator and revive its caller.
    let rt = Runtime::new();
    let exe = compile_script(
        "generator-cancel-yield",
        r#"
        function* f() {
            try { yield 1; __rt.need(5, 3); }
            finally { __rt.effect(2, 8); yield 2; __rt.effect(2, 98); }
        }
        let g = f();
        g.next();
        try { g.next(); __rt.effect(2, 99); }
        finally { __rt.effect(2, 9); }
    "#,
    )
    .unwrap();
    let task = rt.spawn(&exe, &[]).unwrap();
    let mut host = Recorder::default();
    assert_eq!(waiting(run(&task, &mut host)), key(&rt, NUMBER));
    let Cancel::Runnable(cancelled) = task.cancel() else {
        panic!("runnable cancellation")
    };
    assert!(matches!(run(&cancelled, &mut host), RunOutcome::Cancelled));
    assert_eq!(host.reports, [8.0, 9.0]);
}

#[test]
fn getter_waits_on_dependency_and_resumes() {
    // Synthetic fixtures, not light-cljs output. Each row exercises a
    // different property continuation with the same wait/migration/GC path.
    for src in [
        "let o = {get z() { return __rt.need(5, 3) + this.x; }, x: 2}; __rt.effect(1, 3) + o.z + 1",
        "let o = {set z(v) { this.x = __rt.need(5, 3) + v; }}; let before = __rt.effect(1, 3); let assigned = (o.z = 2); before + o.x + assigned - 1",
        "let o = {}; let d = {get value() { return __rt.need(5, 3) + 2; }}; let before = __rt.effect(1, 3); Object.defineProperty(o, 'z', d); before + o.z + 1",
        // The object is retained only by the accumulated descriptor while
        // the later field getter waits, then by the target property.
        "let marker = {n: 9}; let o = {}; let d = {get value() { let v = marker; marker = null; return v; }, get writable() { __rt.need(5, 3); return true; }}; let before = __rt.effect(1, 3); Object.defineProperty(o, 'z', d); before + o.z.n + 1",
        "let o = {set z(v) { this.x = __rt.need(5, 3) + v.n; }}; let before = __rt.effect(1, 3); let assigned = (o.z = {n: 2}); before + o.x + assigned.n - 1",
        "let o = {get valueOf() { let n = __rt.need(5, 3); return () => n + 2; }}; __rt.effect(1, 3) + (+o) + 1",
        "let args = {get length() { return 1; }, get 0() { return __rt.need(5, 3) + 2; }}; __rt.effect(1, 3) + Reflect.apply(x => x, null, args) + 1",
        // The first argument is retained only by the native list cursor
        // across the second index getter's wait.
        "let args = {length: 2, get 0() { return {n: 1}; }, get 1() { return __rt.need(5, 3) + 2; }}; __rt.effect(1, 3) + Reflect.apply((a, b) => a.n + b, null, args)",
        "let it = {}; Object.defineProperty(it, Symbol.iterator, {get: function() { __rt.need(5, 3); return function() { return [9].values(); }; }}); __rt.effect(1, 3) + [...it][0] + 1",
        "let n = 0; let it = {get next() { __rt.need(5, 3); return function() { return {value: 9, done: n++ > 0}; }; }}; it[Symbol.iterator] = function() { return this; }; __rt.effect(1, 3) + [...it][0] + 1",
        "let n = 0; let it = {next() { return {get done() { __rt.need(5, 3); return n > 0; }, get value() { n++; return 9; }}; }}; it[Symbol.iterator] = function() { return this; }; __rt.effect(1, 3) + (function(x) { return x; })(...it) + 1",
        "let n = 0; let it = {next() { return {done: n > 0, get value() { n++; return __rt.need(5, 3) + 2; }}; }}; it[Symbol.iterator] = function() { return this; }; __rt.effect(1, 3) + [...it][0] + 1",
        "let a = {length: 0, set 0(v) { this.x = __rt.need(5, 3) + v; }}; let before = __rt.effect(1, 3); Array.prototype.push.call(a, 2); before + a.x + 1",
        "let f = () => 0; Object.defineProperty(f, 'name', {get: function() { return 'x' + __rt.need(5, 3); }}); let before = __rt.effect(1, 3); let b = f.bind(null); before + (b.name === 'bound x7' ? 10 : 0)",
        "let p = {}; let f = () => 0; Object.defineProperty(f, 'prototype', {get: function() { __rt.need(5, 3); return p; }}); let o = Object.create(p); __rt.effect(1, 3) + (o instanceof f ? 10 : 0)",
    ] {
        let rt = Runtime::new();
        let exe = compile_script("accessor-wait", src).unwrap();
        let task = rt.spawn(&exe, &[]).unwrap();
        let mut host = Recorder::default();
        let expected = key(&rt, NUMBER);
        let mut yielded = 0;
        loop {
            match task.run_with(Budget::instructions(1), &mut host).unwrap() {
                RunOutcome::Yielded => {
                    yielded += 1;
                    assert!(yielded < 1000);
                    rt.collect().unwrap();
                }
                outcome => {
                    assert_eq!(waiting(outcome), expected);
                    break;
                }
            }
        }
        assert_eq!(host.effects, [3.0]);
        let request = one_request(&task, expected);
        rt.collect().unwrap();
        let resumed = only_runnable(publish_number(&rt, &request, 7.0));
        rt.collect().unwrap();
        let (result, host) = thread::spawn(move || (number(run(&resumed, &mut host)), host))
            .join()
            .unwrap();
        assert_eq!(result, 113.0, "{src}");
        assert_eq!(host.effects, [3.0], "no replay: {src}");
        assert_eq!(task.dependency_log().edges(), &[expected]);
    }
}

#[test]
fn accessor_cancellation_and_dependency_failure_unwind() {
    let rt = Runtime::new();
    let exe = compile_script(
        "cancel-accessor",
        "let o = {get z() { try { return __rt.need(5, 3); } finally { __rt.effect(2, 9); } }}; o.z",
    )
    .unwrap();
    let task = rt.spawn(&exe, &[]).unwrap();
    let mut host = Recorder::default();
    let expected = key(&rt, NUMBER);
    assert_eq!(waiting(run(&task, &mut host)), expected);
    let request = one_request(&task, expected);
    let Cancel::Runnable(cancelled) = task.cancel() else {
        panic!("runnable cancellation")
    };
    assert!(matches!(run(&cancelled, &mut host), RunOutcome::Cancelled));
    assert_eq!(host.reports, [9.0]);
    rt.collect().unwrap();
    assert!(
        publish_number(&rt, &request, 7.0)
            .into_runnable()
            .is_empty()
    );

    let rt = Runtime::new();
    let exe = compile_script("failed-accessor", "let o = {get z() { return __rt.need(5, 3); }}; let caught = 0; try { o.z; } catch(e) { caught = 17; } caught").unwrap();
    let task = rt.spawn(&exe, &[]).unwrap();
    assert_eq!(waiting(task.run().unwrap()), key(&rt, NUMBER));
    let request = one_request(&task, key(&rt, NUMBER));
    let error = rt.root(Primitive::Number(42.0));
    let wakeups = rt
        .complete(
            request.key(),
            request.producer(),
            Completion::Failed(&error),
        )
        .unwrap();
    rt.collect().unwrap();
    assert_eq!(number(only_runnable(wakeups).run().unwrap()), 17.0);
}

/// Run a synthetic property fixture with collection at every yield, a
/// pending dependency and resumption on another thread. Both sides of the
/// wait use one-instruction budgets, including native return delivery.
fn resume_property_fixture(src: &str) -> (f64, Recorder) {
    let rt = Runtime::new();
    let task = rt
        .spawn(&compile_script("property-handoff", src).unwrap(), &[])
        .unwrap();
    let expected = key(&rt, NUMBER);
    let mut host = Recorder::default();
    let mut yielded = 0;
    loop {
        match task.run_with(Budget::instructions(1), &mut host).unwrap() {
            RunOutcome::Yielded => {
                yielded += 1;
                assert!(yielded < 1000, "bounded initial execution: {src}");
                rt.collect().unwrap();
            }
            outcome => {
                assert_eq!(waiting(outcome), expected);
                break;
            }
        }
    }
    let request = one_request(&task, expected);
    rt.collect().unwrap();
    let resumed = only_runnable(publish_number(&rt, &request, 7.0));
    rt.collect().unwrap();
    let result = thread::spawn(move || {
        let mut yielded = 0;
        loop {
            match resumed
                .run_with(Budget::instructions(1), &mut host)
                .unwrap()
            {
                RunOutcome::Yielded => {
                    yielded += 1;
                    assert!(yielded < 1000, "bounded resumed execution");
                    rt.collect().unwrap();
                }
                outcome => break (number(outcome), host),
            }
        }
    })
    .join()
    .unwrap();
    assert_eq!(task.dependency_log().edges(), &[expected]);
    result
}

#[test]
fn native_length_conversion_waits_and_resumes() {
    // The fresh length object survives only through the nested conversion
    // continuation once the getter has returned.
    for src in [
        "let list = {get length() { __rt.effect(2, 1); return {valueOf() { __rt.effect(2, 2); return __rt.need(5, 3) - 6; }}; }, get 0() { __rt.effect(2, 3); return 9; }}; __rt.effect(1, 3) + Reflect.apply(x => x, null, list) + 1",
        "let list = {get length() { __rt.effect(2, 1); return {valueOf() { __rt.effect(2, 2); return __rt.need(5, 3) - 6; }}; }, get 0() { __rt.effect(2, 3); return 9; }}; __rt.effect(1, 3) + (x => x).apply(null, list) + 1",
        "let o = {get length() { __rt.effect(2, 1); return {valueOf() { __rt.effect(2, 2); return __rt.need(5, 3) - 7; }}; }, set 0(v) { __rt.effect(2, 3); this.x = v; }, set length(v) { __rt.effect(2, 4); this.len = v; }}; let before = __rt.effect(1, 3); let n = Array.prototype.push.call(o, 9); before + o.x + n",
    ] {
        let (result, host) = resume_property_fixture(src);
        assert_eq!(result, 113.0, "{src}");
        assert_eq!(host.effects, [3.0]);
        let expected: &[f64] = if src.contains("push.call") {
            &[1.0, 2.0, 3.0, 4.0]
        } else {
            &[1.0, 2.0, 3.0]
        };
        assert_eq!(
            host.reports, expected,
            "length getter/conversion and writes run once: {src}"
        );
    }
}

#[test]
fn descriptor_key_conversion_waits_and_resumes() {
    for src in [
        "let o = {}; let k = {get toString() { __rt.effect(2, 1); __rt.need(5, 3); return function() { __rt.effect(2, 2); return 'z'; }; }}; let d = {get get() { __rt.effect(2, 3); return () => 9; }}; let before = __rt.effect(1, 3); Object.defineProperty(o, k, d); before + o.z + 1",
        "let o = {}; let k = {get toString() { __rt.effect(2, 1); return function() { __rt.effect(2, 2); __rt.need(5, 3); return 'z'; }; }}; let d = {get get() { __rt.effect(2, 3); return () => 9; }}; let before = __rt.effect(1, 3); Object.defineProperty(o, k, d); before + o.z + 1",
        "let o = {get z() { return 9; }}; let k = {get toString() { __rt.effect(2, 1); __rt.need(5, 3); return function() { __rt.effect(2, 2); return 'z'; }; }}; let before = __rt.effect(1, 3); let d = Object.getOwnPropertyDescriptor(o, k); __rt.effect(2, 3); before + d.get.call(o) + 1",
        "let o = {get z() { return 9; }}; let k = {get toString() { __rt.effect(2, 1); return function() { __rt.effect(2, 2); __rt.need(5, 3); return 'z'; }; }}; let before = __rt.effect(1, 3); let d = Object.getOwnPropertyDescriptor(o, k); __rt.effect(2, 3); before + d.get.call(o) + 1",
    ] {
        let (result, host) = resume_property_fixture(src);
        assert_eq!(result, 113.0, "{src}");
        assert_eq!(host.effects, [3.0]);
        assert_eq!(
            host.reports,
            [1.0, 2.0, 3.0],
            "conversion precedes descriptor access and runs once: {src}"
        );
    }
}

fn key(rt: &Runtime, (def, kind): (DependencyId, ResultKind)) -> DependencyKey {
    DependencyKey::new(rt.world(), def, kind)
}

/// Host operation 1 (the pre-need effect) returns its argument plus 100;
/// operation 2 reports a value and returns 0. Every call is recorded.
#[derive(Default)]
struct Recorder {
    effects: Vec<f64>,
    reports: Vec<f64>,
}

impl Host for Recorder {
    fn effect(&mut self, call: HostCall) -> Result<Primitive, String> {
        let Primitive::Number(n) = call.arg else {
            return Err(format!("unexpected argument {:?}", call.arg));
        };
        match call.op {
            1 => {
                self.effects.push(n);
                Ok(Primitive::Number(n + 100.0))
            }
            2 => {
                self.reports.push(n);
                Ok(Primitive::Number(0.0))
            }
            op => Err(format!("unexpected host operation {op}")),
        }
    }
}

fn consumer_exe() -> Executable {
    compile_function(include_str!("fixtures/js/need_consumer.js")).unwrap()
}

fn consumer(rt: &Runtime, base: f64) -> Task {
    let base = rt.root(Primitive::Number(base));
    rt.spawn(&consumer_exe(), &[&base]).unwrap()
}

fn run(task: &Task, host: &mut Recorder) -> RunOutcome {
    task.run_with(Budget::UNLIMITED, host).unwrap()
}

fn waiting(outcome: RunOutcome) -> DependencyKey {
    match outcome {
        RunOutcome::Waiting(key) => key,
        other => panic!("expected Waiting, got {other:?}"),
    }
}

fn number(outcome: RunOutcome) -> f64 {
    match outcome {
        RunOutcome::Complete(v) => match v.view() {
            ValueView::Number(n) => n,
            other => panic!("expected a number, got {other:?}"),
        },
        other => panic!("expected Complete, got {other:?}"),
    }
}

fn failed(outcome: RunOutcome) -> VmError {
    match outcome {
        RunOutcome::Failed(e) => e,
        other => panic!("expected Failed, got {other:?}"),
    }
}

/// The one producer request a run claimed, for `expected`.
fn one_request(task: &Task, expected: DependencyKey) -> ProducerRequest {
    let mut requests = task.take_producer_requests();
    assert_eq!(requests.len(), 1, "{requests:?}");
    let request = requests.pop().unwrap();
    assert_eq!(request.key(), expected);
    request
}

/// Publish `n` for `request` and return the wakeups.
fn publish_number(rt: &Runtime, request: &ProducerRequest, n: f64) -> Wakeups {
    let value = rt.root(Primitive::Number(n));
    rt.complete(request.key(), request.producer(), Completion::Ready(&value))
        .unwrap()
}

/// The single task a completion made runnable.
fn only_runnable(wakeups: Wakeups) -> Task {
    let mut runnable = wakeups.into_runnable();
    assert_eq!(runnable.len(), 1);
    runnable.pop().unwrap()
}

/// Source line and column of a failure.
fn line_column(e: &VmError) -> (u32, u32) {
    let at = e.diagnostic().source.expect("source location");
    (at.line, at.column)
}

/// The fixture's line of `__rt.need(5, 3)` and the column of its `(`.
const NUMBER_NEED_AT: (u32, u32) = (12, 34);

/// The whole protocol, driven by hand: the consumer runs its pre-need
/// effect and suspends at a need inside a nested call; the run returns
/// `Waiting` and hands back the producer request; the task is collected
/// around while parked; publication only wakes it; a run on another thread
/// continues at the need (the inner frame's live operand and the outer
/// locals intact), suspends again on a callable produced by another VM task,
/// and finally invokes that callable. The pre-need effect ran once and the
/// function was never restarted.
#[test]
fn a_manually_driven_host_suspends_and_resumes_at_the_need() {
    let rt = Runtime::new();
    let task = consumer(&rt, 3.0);
    let mut host = Recorder::default();

    // Run 1: pre-need effect, then the need of definition 5 is pending.
    let number_key = key(&rt, NUMBER);
    assert_eq!(waiting(run(&task, &mut host)), number_key);
    assert_eq!(task.status(), TaskStatus::Waiting);
    assert_eq!(host.effects, [3.0]);
    let number_request = one_request(&task, number_key);
    assert!(task.take_producer_requests().is_empty(), "taken once");
    let at = task.source_location().expect("suspended at the need");
    assert_eq!((at.line, at.column), NUMBER_NEED_AT);
    let log = task.dependency_log();
    assert_eq!(log.waiting(), Some(number_key));
    assert!(matches!(
        rt.cell_status(number_key),
        CellStatus::Pending { producer: Some(_), ref waiters } if waiters == &[task.id()]
    ));

    // While it waits: a collection keeps its frames (including the binding
    // cell of `lookups` and the closure `lookup`), and nothing can run it.
    rt.collect().unwrap();
    assert!(task.run().is_err());

    // Publication runs no JavaScript inline: it only reports the wake.
    let resumed = only_runnable(publish_number(&rt, &number_request, 7.0));
    assert_eq!(resumed.id(), task.id());
    assert_eq!(task.status(), TaskStatus::Ready);
    assert_eq!((host.effects.len(), host.reports.len()), (1, 0));

    // Run 2, on another thread: continues at the need with `3 * 10` still
    // on the inner frame's operand stack, then suspends on the callable.
    let callable_key = key(&rt, CALLABLE);
    let (outcome, mut host) = thread::spawn(move || {
        let outcome = run(&resumed, &mut host);
        (outcome, host)
    })
    .join()
    .unwrap();
    assert_eq!(waiting(outcome), callable_key);
    assert_eq!(host.effects, [3.0], "the pre-need effect is not replayed");
    assert_eq!(host.reports, [37.0]);
    let callable_request = one_request(&task, callable_key);

    // The producer is another VM task whose result is a function compiled
    // by this runtime; the host publishes it as the callable result.
    let producer_exe = compile_function(include_str!("fixtures/js/need_producer.js")).unwrap();
    let offset = rt.root(Primitive::Number(1000.0));
    let producer = rt.spawn(&producer_exe, &[&offset]).unwrap();
    let RunOutcome::Complete(helper) = producer.run().unwrap() else {
        panic!("the producer returns its function");
    };
    assert_eq!(helper.view(), ValueView::Object);
    let wakeups = rt
        .complete(
            callable_request.key(),
            callable_request.producer(),
            Completion::Ready(&helper),
        )
        .unwrap();
    let resumed = only_runnable(wakeups);
    assert_eq!(task.status(), TaskStatus::Ready);

    // Run 3: the published callable is invoked: pre + helper(37).
    assert_eq!(
        number(run(&resumed, &mut host)),
        103.0 + (37.0 * 2.0 + 1000.0)
    );
    assert_eq!(host.effects, [3.0]);
    assert_eq!(host.reports, [37.0, 1.0], "`lookup` ran once");
    let log = task.dependency_log();
    assert_eq!(log.edges(), &[number_key, callable_key]);
    assert_eq!(log.waiting(), None);
    assert!(task.take_producer_requests().is_empty());
}

/// A template literal (clj-quickjs-57) keeps its partial string across
/// budget yields and a dependency wait: the run suspends at a need in the
/// second substitution with `"a101b"` built, is collected while parked,
/// resumes on another thread, and finishes without replaying the host
/// effect of the first substitution.
#[test]
fn template_literal_resumes_after_wait() {
    let rt = Runtime::new();
    let exe = compile_script("t", "`a${__rt.effect(1, 1)}b${__rt.need(5, 3)}c`").unwrap();
    let task = rt.spawn(&exe, &[]).unwrap();
    let mut host = Recorder::default();
    let number_key = key(&rt, NUMBER);
    let mut runs = 0;
    let request = loop {
        runs += 1;
        assert!(runs < 1_000, "bounded");
        match task.run_with(Budget::instructions(1), &mut host).unwrap() {
            RunOutcome::Yielded => {}
            RunOutcome::Waiting(k) => {
                assert_eq!(k, number_key);
                break one_request(&task, number_key);
            }
            other => panic!("expected the wait, got {other:?}"),
        }
    };
    assert!(runs > 3, "the budget split the run ({runs} runs)");
    assert_eq!(host.effects, [1.0]);
    rt.collect().unwrap();
    let resumed = only_runnable(publish_number(&rt, &request, 42.0));
    let (outcome, host) = thread::spawn(move || {
        let outcome = loop {
            match resumed
                .run_with(Budget::instructions(1), &mut host)
                .unwrap()
            {
                RunOutcome::Yielded => {}
                other => break other,
            }
        };
        (outcome, host)
    })
    .join()
    .unwrap();
    match outcome {
        RunOutcome::Complete(v) => {
            assert_eq!(v.string().expect("a string").to_string(), "a101b42c");
        }
        other => panic!("expected Complete, got {other:?}"),
    }
    assert_eq!(host.effects, [1.0], "the effect is not replayed");
}

/// A property update (clj-quickjs-76) keeps its reference across budget
/// yields and a dependency wait in its computed key: the run suspends at a
/// need in `get()[key]++` with the base on the operand stack, is collected
/// while parked, resumes on another thread one instruction at a time (the
/// postfix old value in its scratch local across yields), and finishes with
/// the base and the key evaluated once and the effect not replayed.
#[test]
fn property_update_resumes_after_wait() {
    let rt = Runtime::new();
    let exe = compile_script(
        "t",
        "let n = 0, m = 0; const p = {k: 1}; \
         const get = () => { m = m + __rt.effect(1, 1) - 100; return p; }; \
         const r = get()[(n = n + 1, __rt.need(5, 3) === 42 ? 'k' : 'z')]++; \
         const s = ++p.k; \
         `${r},${s},${p.k},${p.z},${n},${m}`",
    )
    .unwrap();
    let task = rt.spawn(&exe, &[]).unwrap();
    let mut host = Recorder::default();
    let number_key = key(&rt, NUMBER);
    let mut runs = 0;
    let request = loop {
        runs += 1;
        assert!(runs < 1_000, "bounded");
        match task.run_with(Budget::instructions(1), &mut host).unwrap() {
            RunOutcome::Yielded => {}
            RunOutcome::Waiting(k) => {
                assert_eq!(k, number_key);
                break one_request(&task, number_key);
            }
            other => panic!("expected the wait, got {other:?}"),
        }
    };
    assert!(runs > 3, "the budget split the run ({runs} runs)");
    assert_eq!(host.effects, [1.0]);
    rt.collect().unwrap();
    let resumed = only_runnable(publish_number(&rt, &request, 42.0));
    let (outcome, host) = thread::spawn(move || {
        let mut runs = 0;
        let outcome = loop {
            runs += 1;
            assert!(runs < 1_000, "bounded");
            match resumed
                .run_with(Budget::instructions(1), &mut host)
                .unwrap()
            {
                RunOutcome::Yielded => {}
                other => break other,
            }
        };
        (outcome, host)
    })
    .join()
    .unwrap();
    match outcome {
        RunOutcome::Complete(v) => {
            assert_eq!(
                v.string().expect("a string").to_string(),
                "1,3,3,undefined,1,1"
            );
        }
        other => panic!("expected Complete, got {other:?}"),
    }
    assert_eq!(host.effects, [1.0], "the effect is not replayed");
}

/// A compound assignment on a property (clj-quickjs-260) keeps its
/// reference and the value it read across budget yields and a dependency
/// wait in its right operand: the run suspends at a need in
/// `get()[key] += ...` with the base, the converted key and the old value
/// on the operand stack, is collected while parked, resumes on another
/// thread one instruction at a time, and finishes with the base and the key
/// evaluated once, the old value used, and the effect not replayed.
#[test]
fn property_compound_assignment_resumes_after_wait() {
    let rt = Runtime::new();
    let exe = compile_script(
        "t",
        "let n = 0, m = 0; const p = {k: 'v'}; \
         const get = () => { m = m + __rt.effect(1, 1) - 100; return p; }; \
         const r = get()[(n = n + 1, 'k')] += (p.k = 'w', __rt.need(5, 3)); \
         p.k *= 1; \
         `${r},${p.k},${n},${m}`",
    )
    .unwrap();
    let task = rt.spawn(&exe, &[]).unwrap();
    let mut host = Recorder::default();
    let number_key = key(&rt, NUMBER);
    let mut runs = 0;
    let request = loop {
        runs += 1;
        assert!(runs < 1_000, "bounded");
        match task.run_with(Budget::instructions(1), &mut host).unwrap() {
            RunOutcome::Yielded => {}
            RunOutcome::Waiting(k) => {
                assert_eq!(k, number_key);
                break one_request(&task, number_key);
            }
            other => panic!("expected the wait, got {other:?}"),
        }
    };
    assert!(runs > 3, "the budget split the run ({runs} runs)");
    assert_eq!(host.effects, [1.0]);
    rt.collect().unwrap();
    let resumed = only_runnable(publish_number(&rt, &request, 42.0));
    let (outcome, host) = thread::spawn(move || {
        let mut runs = 0;
        let outcome = loop {
            runs += 1;
            assert!(runs < 1_000, "bounded");
            match resumed
                .run_with(Budget::instructions(1), &mut host)
                .unwrap()
            {
                RunOutcome::Yielded => {}
                other => break other,
            }
        };
        (outcome, host)
    })
    .join()
    .unwrap();
    match outcome {
        RunOutcome::Complete(v) => {
            // `'v' + 42`, written over the right operand's `'w'`, then
            // `'v42' * 1` is NaN.
            assert_eq!(v.string().expect("a string").to_string(), "v42,NaN,1,1");
        }
        other => panic!("expected Complete, got {other:?}"),
    }
    assert_eq!(host.effects, [1.0], "the effect is not replayed");
}

/// A need of a completed cell continues in the same run: no suspension, no
/// producer request, and the read is still recorded.
#[test]
fn a_ready_read_continues_in_the_same_run() {
    let rt = Runtime::new();
    let number_request = rt.claim(key(&rt, NUMBER)).unwrap();
    assert!(
        publish_number(&rt, &number_request, 7.0)
            .deliveries()
            .is_empty()
    );
    let callable = rt.claim(key(&rt, CALLABLE)).unwrap();
    let producer_exe = compile_function(include_str!("fixtures/js/need_producer.js")).unwrap();
    let offset = rt.root(Primitive::Number(-5.0));
    let RunOutcome::Complete(helper) = rt.spawn(&producer_exe, &[&offset]).unwrap().run().unwrap()
    else {
        panic!("the producer returns its function");
    };
    let wakeups = rt
        .complete(
            callable.key(),
            callable.producer(),
            Completion::Ready(&helper),
        )
        .unwrap();
    assert!(wakeups.deliveries().is_empty());

    let task = consumer(&rt, 2.0);
    let mut host = Recorder::default();
    // 2 + 100 + helper(2 * 10 + 7) = 102 + 54 - 5
    assert_eq!(number(run(&task, &mut host)), 151.0);
    assert_eq!(
        (host.effects.as_slice(), host.reports.as_slice()),
        (&[2.0][..], &[27.0, 1.0][..])
    );
    assert!(task.take_producer_requests().is_empty());
    let log = task.dependency_log();
    assert_eq!(log.edges(), &[key(&rt, NUMBER), key(&rt, CALLABLE)]);
    assert_eq!(log.waiting(), None);
}

/// Budgeted runs cut the consumer into many pieces; the need's suspension
/// is one of them, and no piece replays work.
#[test]
fn budgeted_runs_and_the_wait_never_replay_work() {
    let rt = Runtime::new();
    let task = consumer(&rt, 4.0);
    let mut host = Recorder::default();
    let mut runs = 0;
    let number_key = key(&rt, NUMBER);
    let mut requests = Vec::new();
    let result = loop {
        runs += 1;
        assert!(runs < 10_000, "bounded");
        match task.run_with(Budget::instructions(2), &mut host).unwrap() {
            RunOutcome::Yielded => {}
            RunOutcome::Waiting(k) => {
                requests.extend(task.take_producer_requests());
                let request = requests.pop().expect("one request per pending key");
                assert_eq!(request.key(), k);
                if k == number_key {
                    // The wake makes it runnable; the loop runs it again.
                    let _ = only_runnable(publish_number(&rt, &request, 1.0));
                } else {
                    let exe =
                        compile_function(include_str!("fixtures/js/need_producer.js")).unwrap();
                    let offset = rt.root(Primitive::Number(0.0));
                    let RunOutcome::Complete(helper) =
                        rt.spawn(&exe, &[&offset]).unwrap().run().unwrap()
                    else {
                        panic!("the producer returns its function");
                    };
                    let wakeups = rt
                        .complete(k, request.producer(), Completion::Ready(&helper))
                        .unwrap();
                    let _ = only_runnable(wakeups);
                }
            }
            other => break number(other),
        }
    };
    // 4 + 100 + helper(41) = 104 + 82
    assert_eq!(result, 186.0);
    assert_eq!(
        (host.effects.as_slice(), host.reports.as_slice()),
        (&[4.0][..], &[41.0, 1.0][..])
    );
    assert!(runs > 10, "the budget split the run ({runs} runs)");
}

/// A published failure reaches the run failure boundary at the need, with
/// the need's source location, whether the failure was already published
/// (same run) or arrives while the task waits (resumed run).
#[test]
fn a_failed_dependency_fails_the_run_at_the_need() {
    let rt = Runtime::new();
    let number_key = key(&rt, NUMBER);
    let request = rt.claim(number_key).unwrap();
    let error = rt.root(Primitive::Number(13.0));
    let _ = rt
        .complete(number_key, request.producer(), Completion::Failed(&error))
        .unwrap();
    let mut host = Recorder::default();
    let e = failed(run(&consumer(&rt, 1.0), &mut host));
    assert_eq!(e.kind(), VmErrorKind::DependencyFailed(number_key));
    assert!(e.message().contains("failed with Number(13.0)"), "{e}");
    assert_eq!(line_column(&e), NUMBER_NEED_AT);
    assert_eq!(e.diagnostic().opcode, Some(clj_quickjs::Opcode::Need));
    assert_eq!(e.diagnostic().call_chain.len(), 1, "inside `lookup`");

    // Published while the consumer waits.
    let rt = Runtime::new();
    let number_key = key(&rt, NUMBER);
    let task = consumer(&rt, 1.0);
    let mut host = Recorder::default();
    assert_eq!(waiting(run(&task, &mut host)), number_key);
    let request = one_request(&task, number_key);
    let error = rt.root(Primitive::Number(14.0));
    let wakeups = rt
        .complete(number_key, request.producer(), Completion::Failed(&error))
        .unwrap();
    let e = failed(run(&only_runnable(wakeups), &mut host));
    assert_eq!(e.kind(), VmErrorKind::DependencyFailed(number_key));
    assert_eq!(line_column(&e), NUMBER_NEED_AT);
    assert_eq!(task.status(), TaskStatus::Finished);
    assert_eq!(host.effects, [1.0]);
}

/// A cancelled cell fails the needing run (the task itself is not
/// cancelled); a cancelled consumer ends `Cancelled` and leaves the cell and
/// its producer to the other consumers.
#[test]
fn cell_and_consumer_cancellation_reach_the_run_boundary() {
    // The embedder cancels the cell while the consumer waits.
    let rt = Runtime::new();
    let number_key = key(&rt, NUMBER);
    let task = consumer(&rt, 1.0);
    let mut host = Recorder::default();
    assert_eq!(waiting(run(&task, &mut host)), number_key);
    let _request = one_request(&task, number_key);
    let e = failed(run(
        &only_runnable(rt.cancel_cell(number_key).unwrap()),
        &mut host,
    ));
    assert_eq!(e.kind(), VmErrorKind::DependencyCancelled(number_key));
    assert_eq!(line_column(&e), NUMBER_NEED_AT);
    assert_eq!(task.status(), TaskStatus::Finished);

    // A parked consumer is cancelled; another consumer of the same key keeps
    // waiting and the producer request (claimed by the first) still settles
    // the cell for it.
    let rt = Runtime::new();
    let number_key = key(&rt, NUMBER);
    let (first, second) = (consumer(&rt, 1.0), consumer(&rt, 2.0));
    let mut host = Recorder::default();
    assert_eq!(waiting(run(&first, &mut host)), number_key);
    assert_eq!(waiting(run(&second, &mut host)), number_key);
    let request = one_request(&first, number_key);
    assert!(
        second.take_producer_requests().is_empty(),
        "one producer per cell"
    );
    let Cancel::Runnable(cancelled) = first.cancel() else {
        panic!("a parked consumer is handed back to run its cleanup");
    };
    assert!(matches!(run(&cancelled, &mut host), RunOutcome::Cancelled));
    assert_eq!(first.status(), TaskStatus::Cancelled);
    assert_eq!(first.dependency_log().waiting(), None);
    let resumed = only_runnable(publish_number(&rt, &request, 7.0));
    assert_eq!(resumed.id(), second.id());
    assert_eq!(waiting(run(&resumed, &mut host)), key(&rt, CALLABLE));
    assert_eq!(host.reports, [27.0]);

    // Cancelled before its need (by its own pre-need effect): the need is
    // refused, so nothing is subscribed or claimed, and the run ends
    // `Cancelled`.
    struct CancelSelf(Option<Task>);
    impl Host for CancelSelf {
        fn effect(&mut self, _: HostCall) -> Result<Primitive, String> {
            let task = self.0.take().expect("one effect");
            assert!(matches!(task.cancel(), Cancel::Requested));
            Ok(Primitive::Number(0.0))
        }
    }
    let rt = Runtime::new();
    let task = consumer(&rt, 1.0);
    let outcome = task
        .run_with(Budget::UNLIMITED, &mut CancelSelf(Some(task.clone())))
        .unwrap();
    assert!(matches!(outcome, RunOutcome::Cancelled), "{outcome:?}");
    assert_eq!(task.status(), TaskStatus::Cancelled);
    assert!(matches!(
        rt.cell_status(key(&rt, NUMBER)),
        CellStatus::Absent
    ));
    assert!(task.take_producer_requests().is_empty());
}

/// The world is execution context: the same source in another world needs
/// that world's key, and a result published in the first world does not
/// satisfy it.
#[test]
fn the_need_reads_the_task_world() {
    let rt = Runtime::new();
    let first = rt.world();
    let second = rt.new_world();
    let request = rt.claim(key(&rt, NUMBER)).unwrap();
    let _ = publish_number(&rt, &request, 7.0);
    let base = rt.root(Primitive::Number(1.0));
    let task = rt.spawn_in(second, &consumer_exe(), &[&base]).unwrap();
    let k = waiting(run(&task, &mut Recorder::default()));
    assert_eq!(k, DependencyKey::new(second, NUMBER.0, NUMBER.1));
    assert_ne!(k.world(), first);
}

/// `__rt.need` compiles to the `need` instruction; its operands are checked
/// at compile time when literal and at run time otherwise.
#[test]
fn need_operands_are_validated() {
    let exe = compile_script("t", "__rt.need(1, 2);").unwrap();
    // const 0, const 1, need, …: no operand byte of this program is 0x36.
    assert!(
        exe.to_spec()
            .code
            .contains(&(clj_quickjs::Opcode::Need as u8))
    );
    for (src, column) in [
        ("__rt.need(1.5, 2);", 11),
        ("__rt.need(1, 4294967296);", 14),
        ("__rt.need(9007199254740992, 1);", 11),
    ] {
        let e = compile_script("t", src).expect_err(src);
        assert_eq!(
            (e.kind, e.line, e.column),
            (SyntaxErrorKind::Invalid, 1, column),
            "{src}"
        );
    }
    let rt = Runtime::new();
    for src in [
        "__rt.need(0 - 1, 2);",
        "__rt.need(1, 0.5 + 0);",
        "__rt.need(true, 2);",
        "__rt.need(1, __rt.vector());",
    ] {
        let task = rt.spawn(&compile_script("t", src).unwrap(), &[]).unwrap();
        let e = failed(task.run().unwrap());
        assert_eq!(e.kind(), VmErrorKind::UnsupportedOperand, "{src}");
        assert!(e.message().contains("non-negative integer"), "{e}");
        assert_eq!(line_column(&e), (1, 10), "{src}: at the `(`");
    }
    // Inside a loop, a ready need is read on every iteration, recorded once.
    let rt = Runtime::new();
    let request = rt
        .claim(DependencyKey::new(
            rt.world(),
            DependencyId::new(1),
            ResultKind::new(2),
        ))
        .unwrap();
    let _ = publish_number(&rt, &request, 5.0);
    let src =
        "let i = 0; let sum = 0; while (i < 3) { sum = sum + __rt.need(1, 2); i = i + 1; } sum;";
    let task = rt.spawn(&compile_script("t", src).unwrap(), &[]).unwrap();
    assert_eq!(number(task.run().unwrap()), 15.0);
    assert_eq!(task.dependency_log().edges().len(), 1);
}

/// A published persistent graph reaches consumers through `need` as the
/// very objects the producer published, not copies, whether the consumer
/// waited for it or read it ready, and stays alive through its cell alone
/// after the producer drops its handle and collections run.
#[test]
fn a_published_object_keeps_its_identity_through_need() {
    let rt = Runtime::new();
    let k = DependencyKey::new(rt.world(), DependencyId::new(9), ResultKind::new(1));
    let (graph, leaf) = rt.scope(|s| {
        let leaf = s
            .new_vector(&[s.primitive(Primitive::Number(1.0))])
            .unwrap();
        let outer = s
            .new_vector(&[leaf, s.primitive(Primitive::Number(2.0))])
            .unwrap();
        (s.persist(outer), s.downgrade(leaf))
    });
    // Returns the needed graph and its first element.
    let exe = compile_script(
        "t",
        "const g = __rt.need(9, 1); __rt.vector(g, __rt.nth(g, 0));",
    )
    .unwrap();
    let waiter = rt.spawn(&exe, &[]).unwrap();
    assert_eq!(waiting(waiter.run().unwrap()), k);
    let request = one_request(&waiter, k);
    let wakeups = rt
        .complete(k, request.producer(), Completion::Ready(&graph))
        .unwrap();
    let graph_weak = graph.downgrade();
    drop(graph);
    rt.collect().unwrap();
    assert!(graph_weak.is_live() && leaf.is_live(), "the cell keeps it");

    let reader = rt.spawn(&exe, &[]).unwrap();
    for task in [only_runnable(wakeups), reader] {
        let RunOutcome::Complete(result) = task.run().unwrap() else {
            panic!("the consumer returns the pair");
        };
        rt.collect().unwrap();
        rt.scope(|s| {
            let items = s.items(s.local(&result).unwrap()).unwrap();
            assert!(s.same_object(items[0], s.upgrade(&graph_weak).unwrap()));
            assert!(s.same_object(items[1], s.upgrade(&leaf).unwrap()));
        });
    }
}

/// A method call (clj-quickjs-117) keeps its receiver below the callee
/// while an argument waits: the receiver is a temporary object only the
/// suspended frame holds, it survives a collection during the wait, and
/// after publication the run on another thread passes the very object to
/// the built-in `valueOf`. A JavaScript method gets its argument the same
/// way (its receiver becomes the frame's `this`, clj-quickjs-116). Each
/// `make` ran once.
#[test]
fn method_calls_preserve_receiver_across_wait_and_collection() {
    let rt = Runtime::new();
    let exe = compile_script(
        "method_need",
        "function make(n) { __rt.effect(1, n); \
           return {x: 5, m: Object.prototype.valueOf, j: function (a) { return a * 2; }}; } \
         let r = make(1).m(__rt.need(5, 3)); \
         let j = make(2).j(__rt.need(5, 3)); \
         r.x * 100 + j",
    )
    .unwrap();
    let task = rt.spawn(&exe, &[]).unwrap();
    let mut host = Recorder::default();
    let number_key = key(&rt, NUMBER);
    assert_eq!(waiting(run(&task, &mut host)), number_key);
    assert_eq!(host.effects, [1.0]);
    let request = one_request(&task, number_key);
    // Only the suspended frame holds the receiver.
    rt.collect().unwrap();
    rt.collect().unwrap();
    let resumed = only_runnable(publish_number(&rt, &request, 7.0));
    let (outcome, host) = thread::spawn(move || {
        let outcome = run(&resumed, &mut host);
        (outcome, host)
    })
    .join()
    .unwrap();
    assert_eq!(number(outcome), 5.0 * 100.0 + 14.0);
    assert_eq!(host.effects, [1.0, 2.0], "each make ran once");
}

/// Effects of every worker of [`migrating_executor`]: (worker, argument),
/// in execution order.
type WorkerEffects = Arc<Mutex<Vec<(WorkerId, f64)>>>;

/// Logs host operation 1 with its worker and returns its argument.
struct WorkerLog {
    worker: WorkerId,
    effects: WorkerEffects,
}

impl Host for WorkerLog {
    fn effect(&mut self, call: HostCall) -> Result<Primitive, String> {
        match (call.op, &call.arg) {
            (1, &Primitive::Number(n)) => {
                self.effects.lock().unwrap().push((self.worker, n));
                Ok(call.arg)
            }
            other => Err(format!("unexpected host effect {other:?}")),
        }
    }
}

/// The two-worker executor, with woken tasks resuming on the worker after
/// the one they parked on.
fn migrating_executor(rt: &Runtime, effects: &WorkerEffects) -> Executor {
    let effects = Arc::clone(effects);
    ExecutorConfig::new()
        .workers(2)
        .host(move |worker| {
            Box::new(WorkerLog {
                worker,
                effects: Arc::clone(&effects),
            })
        })
        .placement(|r| match (r.reason, r.last_worker) {
            (EnqueueReason::Woken, Some(last)) => {
                Placement::Worker(WorkerId::new((last.index() + 1) % r.workers))
            }
            (_, Some(last)) => Placement::Worker(last),
            _ => Placement::Any,
        })
        .start(rt)
        .unwrap()
}

/// Synthetic fixture, not light-cljs output. The exact accumulator already
/// contains 1e16 when next suspends, and must survive GC and worker migration.
#[test]
fn math_sum_precise_waits_and_migrates() {
    let rt = Runtime::new();
    let effects = WorkerEffects::default();
    let exec = migrating_executor(&rt, &effects);
    let src = "let n = 0, o = {next() { n++; __rt.effect(1, n); if (n === 2) __rt.need(5, 3); return {value: n === 1 ? 1e16 : n === 2 ? 1 : -1e16, done: n > 3}; }}; o[Symbol.iterator] = function() { return this; }; Math.sumPrecise(o) === 1 && n === 4";
    let task = rt
        .spawn(&compile_script("sum-precise-wait", src).unwrap(), &[])
        .unwrap();
    exec.submit_with(task.clone(), Placement::Worker(WorkerId::new(0)))
        .unwrap();
    let expected = key(&rt, NUMBER);
    assert!(exec.wait_for(TIMEOUT, |events| {
        events.iter().any(|e| {
            e.task == Some(task.id())
                && e.kind == EventKind::Released(OutcomeKind::Waiting(expected))
        })
    }));
    collect_on_worker(&exec, WorkerId::new(0));
    collect_on_worker(&exec, WorkerId::new(1));
    let request = one_request(&task, expected);
    assert_eq!(exec.wake(publish_number(&rt, &request, 7.0)).unwrap(), 1);
    let RunOutcome::Complete(value) = exec.take_outcome(task.id(), TIMEOUT).unwrap() else {
        panic!("sum did not complete")
    };
    assert_eq!(value.view(), ValueView::Bool(true));
    let (w0, w1) = (WorkerId::new(0), WorkerId::new(1));
    assert_eq!(
        *effects.lock().unwrap(),
        [(w0, 1.0), (w0, 2.0), (w1, 3.0), (w1, 4.0)]
    );
    assert_eq!(task.dependency_log().edges(), &[expected]);
    exec.shutdown(TIMEOUT).unwrap();
}

/// Synthetic fixtures, not light-cljs output. One-instruction slices collect
/// between every native action, including acquisition and abrupt cleanup.
#[test]
fn math_sum_precise_getters_and_close_wait() {
    for src in [
        "let n = 0, o = {}; Object.defineProperty(o, Symbol.iterator, {get() { n++; __rt.effect(1, 3); __rt.need(5, 3); return function() { return [1e16, 1, -1e16].values(); }; }}); Math.sumPrecise(o) === 1 && n === 1",
        "let n = 0, o = {get next() { __rt.effect(1, 3); __rt.need(5, 3); return function() { return {value: 1, done: n++ > 0}; }; }}; o[Symbol.iterator] = function() { return this; }; Math.sumPrecise(o) === 1 && n === 2",
        "let n = 0, o = {next() { return {get done() { n++; if (n === 1) { __rt.effect(1, 3); __rt.need(5, 3); } return n > 1; }, value: 1}; }}; o[Symbol.iterator] = function() { return this; }; Math.sumPrecise(o) === 1 && n === 2",
        "let n = 0, o = {next() { return ++n === 1 ? {get value() { __rt.effect(1, 3); return __rt.need(5, 3); }} : {done: true}; }}; o[Symbol.iterator] = function() { return this; }; Math.sumPrecise(o) === 7 && n === 2",
        "let n = 0, c = 0, o = {next() { n++; return {value: 'bad'}; }, return() { c++; __rt.effect(1, 3); __rt.need(5, 3); throw 99; }}; o[Symbol.iterator] = function() { return this; }; let ok = false; try { Math.sumPrecise(o); } catch(e) { ok = e instanceof TypeError; } ok && n === 1 && c === 1",
        "let c = 0, o = {next() { return {value: {}}; }, get return() { c++; __rt.effect(1, 3); __rt.need(5, 3); throw 99; }}; o[Symbol.iterator] = function() { return this; }; let ok = false; try { Math.sumPrecise(o); } catch(e) { ok = e instanceof TypeError; } ok && c === 1",
    ] {
        let rt = Runtime::new();
        let task = rt
            .spawn(&compile_script("sum-precise-getters", src).unwrap(), &[])
            .unwrap();
        let expected = key(&rt, NUMBER);
        let mut host = Recorder::default();
        let mut reached_wait = false;
        for _ in 0..3000 {
            match task.run_with(Budget::instructions(1), &mut host).unwrap() {
                RunOutcome::Yielded => {
                    rt.collect().unwrap();
                }
                outcome => {
                    assert_eq!(waiting(outcome), expected);
                    reached_wait = true;
                    break;
                }
            }
        }
        assert!(reached_wait, "{src}");
        let request = one_request(&task, expected);
        rt.collect().unwrap();
        let resumed = only_runnable(publish_number(&rt, &request, 7.0));
        let (result, host) = thread::spawn(move || {
            for _ in 0..3000 {
                match resumed
                    .run_with(Budget::instructions(1), &mut host)
                    .unwrap()
                {
                    RunOutcome::Yielded => {
                        rt.collect().unwrap();
                    }
                    outcome => return (outcome, host),
                }
            }
            panic!("sumPrecise did not complete");
        })
        .join()
        .unwrap();
        let RunOutcome::Complete(value) = result else {
            panic!("unexpected {result:?}: {src}");
        };
        assert_eq!(value.view(), ValueView::Bool(true), "{src}");
        assert_eq!(host.effects, [3.0], "no replay: {src}");
        assert_eq!(task.dependency_log().edges(), &[expected]);
    }
}

/// Request a collection on `worker` and wait until it is recorded.
fn collect_on_worker(exec: &Executor, worker: WorkerId) {
    let collections = |ev: &[Event]| {
        ev.iter()
            .filter(|e| matches!(e.kind, EventKind::Collected(_)))
            .count()
    };
    let before = collections(&exec.events());
    exec.schedule_collection(Placement::Worker(worker));
    assert!(exec.wait_for(TIMEOUT, |ev| collections(ev) > before));
}

const TIMEOUT: Duration = Duration::from_secs(30);

/// A method's `this` (clj-quickjs-116) is a traced slot of its task-owned
/// frame: the method writes a property through `this`, suspends at
/// `__rt.need` on a pending cell, and is collected around on both workers
/// while only that slot holds the receiver (a temporary object). After
/// publication the task resumes on the other worker and reads `this.x` and
/// the property it wrote from the same object. Each effect ran once.
#[test]
fn a_method_resumes_with_the_same_this_after_a_wait_and_migration() {
    for concise in [false, true] {
        method_resumes_with_this(concise);
    }
}

fn method_resumes_with_this(concise: bool) {
    let rt = Runtime::new();
    let effects = WorkerEffects::default();
    let exec = migrating_executor(&rt, &effects);
    let source = "function make() {
           return {x: 5, m: function (a) {
             this.y = __rt.effect(1, a);
             let v = __rt.need(5, 3);
             return this.x * 1000 + this.y * 10 + v + __rt.effect(1, 2);
           }};
         }
         make().m(1)";
    let source = if concise {
        source.replace("m: function (a)", "m(a)")
    } else {
        source.to_string()
    };
    let exe = compile_script("this_need", &source).unwrap();
    let task = rt.spawn(&exe, &[]).unwrap();
    exec.submit_with(task.clone(), Placement::Worker(WorkerId::new(0)))
        .unwrap();
    let parked = OutcomeKind::Waiting(key(&rt, NUMBER));
    assert!(exec.wait_for(TIMEOUT, |ev| {
        ev.iter()
            .any(|e| e.task == Some(task.id()) && e.kind == EventKind::Released(parked))
    }));
    collect_on_worker(&exec, WorkerId::new(0));
    collect_on_worker(&exec, WorkerId::new(1));
    assert_eq!(task.status(), TaskStatus::Waiting);

    let request = one_request(&task, key(&rt, NUMBER));
    assert_eq!(exec.wake(publish_number(&rt, &request, 7.0)).unwrap(), 1);
    let outcome = exec.take_outcome(task.id(), TIMEOUT).expect("an outcome");
    assert_eq!(number(outcome), 5.0 * 1000.0 + 10.0 + 7.0 + 2.0);
    let (w0, w1) = (WorkerId::new(0), WorkerId::new(1));
    assert_eq!(*effects.lock().unwrap(), [(w0, 1.0), (w1, 2.0)]);
    exec.shutdown(TIMEOUT).unwrap();
}

/// Synthetic fixture, not light-cljs output. The constructor has returned;
/// only the arrow's value capture retains its receiver across the wait.
#[test]
fn an_arrow_retains_lexical_this_after_a_wait_collection_and_migration() {
    let rt = Runtime::new();
    let effects = WorkerEffects::default();
    let exec = migrating_executor(&rt, &effects);
    let exe = compile_script(
        "arrow_this_need",
        "function F() {
           this.x = 5;
           return () => {
             __rt.effect(1, 1);
             let v = __rt.need(5, 3);
             return this.x * 100 + v + __rt.effect(1, 2);
           };
         }
         const f = new F();
         f.call({x: 99})",
    )
    .unwrap();
    let task = rt.spawn(&exe, &[]).unwrap();
    exec.submit_with(task.clone(), Placement::Worker(WorkerId::new(0)))
        .unwrap();
    let parked = OutcomeKind::Waiting(key(&rt, NUMBER));
    assert!(exec.wait_for(TIMEOUT, |ev| {
        ev.iter()
            .any(|e| e.task == Some(task.id()) && e.kind == EventKind::Released(parked))
    }));
    collect_on_worker(&exec, WorkerId::new(0));
    collect_on_worker(&exec, WorkerId::new(1));
    let request = one_request(&task, key(&rt, NUMBER));
    assert_eq!(exec.wake(publish_number(&rt, &request, 7.0)).unwrap(), 1);
    assert_eq!(
        number(exec.take_outcome(task.id(), TIMEOUT).expect("an outcome")),
        509.0
    );
    assert_eq!(
        *effects.lock().unwrap(),
        [(WorkerId::new(0), 1.0), (WorkerId::new(1), 2.0)]
    );
    exec.shutdown(TIMEOUT).unwrap();
}

/// A construct frame (clj-quickjs-71) is task-owned state like any frame:
/// the constructor writes through `this` (the new object, held only by the
/// frame's traced `this` slot), suspends at `__rt.need` on a pending cell,
/// and is collected around on both workers. After publication it resumes
/// on the other worker, finishes, and its non-object `return 5` is
/// replaced by the same new object, which still inherits from
/// `F.prototype`. Each effect ran once.
#[test]
fn a_constructor_resumes_with_its_new_object_after_a_wait_and_migration() {
    let rt = Runtime::new();
    let effects = WorkerEffects::default();
    let exec = migrating_executor(&rt, &effects);
    let exe = compile_script(
        "construct_need",
        "function F(a) {
           this.y = __rt.effect(1, a);
           let v = __rt.need(5, 3);
           this.v = v + __rt.effect(1, 2);
           return 5;
         }
         F.prototype.get = function () { return this.y * 100 + this.v; };
         const o = new F(1);
         o.constructor === F ? o.get() : -1",
    )
    .unwrap();
    let task = rt.spawn(&exe, &[]).unwrap();
    exec.submit_with(task.clone(), Placement::Worker(WorkerId::new(0)))
        .unwrap();
    let parked = OutcomeKind::Waiting(key(&rt, NUMBER));
    assert!(exec.wait_for(TIMEOUT, |ev| {
        ev.iter()
            .any(|e| e.task == Some(task.id()) && e.kind == EventKind::Released(parked))
    }));
    collect_on_worker(&exec, WorkerId::new(0));
    collect_on_worker(&exec, WorkerId::new(1));
    assert_eq!(task.status(), TaskStatus::Waiting);

    let request = one_request(&task, key(&rt, NUMBER));
    assert_eq!(exec.wake(publish_number(&rt, &request, 7.0)).unwrap(), 1);
    let outcome = exec.take_outcome(task.id(), TIMEOUT).expect("an outcome");
    assert_eq!(number(outcome), 100.0 + 7.0 + 2.0);
    let (w0, w1) = (WorkerId::new(0), WorkerId::new(1));
    assert_eq!(*effects.lock().unwrap(), [(w0, 1.0), (w1, 2.0)]);
    exec.shutdown(TIMEOUT).unwrap();
}

/// Synthetic fixture, not light-cljs output. The right object is reachable
/// only through the pending conversion while the left callback waits.
#[test]
fn to_primitive_waits_yields_collects_and_migrates_without_replay() {
    let rt = Runtime::new();
    let exe = compile_script(
        "conversion",
        r#"
        ({valueOf: function () {
            __rt.effect(2, 1);
            return __rt.need(5, 3);
        }}) - ({n: 4, valueOf: function () {
            __rt.effect(2, 2);
            this.n = __rt.need(6, 2);
            return {};
        }, toString: function () {
            __rt.effect(2, 3);
            return this.n;
        }})
    "#,
    )
    .unwrap();
    let task = rt.spawn(&exe, &[]).unwrap();
    let mut host = Recorder::default();
    let mut yields = 0;
    loop {
        match task.run_with(Budget::instructions(1), &mut host).unwrap() {
            RunOutcome::Yielded => {
                yields += 1;
                assert!(yields < 200);
                rt.collect().unwrap();
            }
            outcome => {
                assert_eq!(waiting(outcome), key(&rt, NUMBER));
                break;
            }
        }
    }
    assert_eq!(host.reports, [1.0]);
    rt.collect().unwrap();
    let request = one_request(&task, key(&rt, NUMBER));
    let resumed = only_runnable(publish_number(&rt, &request, 10.0));
    rt.collect().unwrap();
    let worker_rt = rt.clone();
    let (outcome, mut host) = thread::spawn(move || {
        for _ in 0..200 {
            let outcome = resumed
                .run_with(Budget::instructions(1), &mut host)
                .unwrap();
            if matches!(outcome, RunOutcome::Yielded) {
                worker_rt.collect().unwrap();
            } else {
                return (outcome, host);
            }
        }
        panic!("conversion did not finish within 200 instructions");
    })
    .join()
    .unwrap();
    assert_eq!(waiting(outcome), key(&rt, CALLABLE));
    assert_eq!(
        host.reports,
        [1.0, 2.0],
        "the left conversion finished once"
    );
    rt.collect().unwrap();
    let request = one_request(&task, key(&rt, CALLABLE));
    let resumed = only_runnable(publish_number(&rt, &request, 4.0));
    let (outcome, host) = thread::spawn(move || {
        for _ in 0..200 {
            let outcome = resumed
                .run_with(Budget::instructions(1), &mut host)
                .unwrap();
            if matches!(outcome, RunOutcome::Yielded) {
                rt.collect().unwrap();
            } else {
                return (outcome, host);
            }
        }
        panic!("conversion did not finish within 200 instructions");
    })
    .join()
    .unwrap();
    assert_eq!(number(outcome), 6.0);
    assert_eq!(host.reports, [1.0, 2.0, 3.0]);
    assert!(yields > 10);
}

/// A pending property-key conversion must trace both the base and the
/// value to be assigned, not merely its method receiver.
#[test]
fn to_primitive_property_key_wait_preserves_assignment_operands() {
    let rt = Runtime::new();
    let exe = compile_script(
        "key",
        r#"
        let o = {};
        o[{toString: function () {
            __rt.effect(2, 1);
            __rt.need(5, 3);
            return 'x';
        }}] = {n: 7};
        o.x.n;
    "#,
    )
    .unwrap();
    let task = rt.spawn(&exe, &[]).unwrap();
    let mut host = Recorder::default();
    assert_eq!(waiting(run(&task, &mut host)), key(&rt, NUMBER));
    assert_eq!(host.reports, [1.0]);
    rt.collect().unwrap();
    let request = one_request(&task, key(&rt, NUMBER));
    let resumed = only_runnable(publish_number(&rt, &request, 0.0));
    let (outcome, host) = thread::spawn(move || {
        rt.collect().unwrap();
        (run(&resumed, &mut host), host)
    })
    .join()
    .unwrap();
    assert_eq!(number(outcome), 7.0);
    assert_eq!(host.reports, [1.0]);
}

#[test]
fn to_primitive_bound_method_waits_with_its_bound_receiver() {
    let rt = Runtime::new();
    let exe = compile_script(
        "bound conversion",
        r#"
        function method(offset) {
            __rt.effect(2, this.n);
            return offset + this.n + __rt.need(5, 3);
        }
        let o = {n: 99};
        o.valueOf = method.bind({n: 4}, 3);
        o * 2;
    "#,
    )
    .unwrap();
    let task = rt.spawn(&exe, &[]).unwrap();
    let mut host = Recorder::default();
    assert_eq!(waiting(run(&task, &mut host)), key(&rt, NUMBER));
    assert_eq!(host.reports, [4.0]);
    rt.collect().unwrap();
    let request = one_request(&task, key(&rt, NUMBER));
    let resumed = only_runnable(publish_number(&rt, &request, 2.0));
    let (outcome, host) = thread::spawn(move || {
        rt.collect().unwrap();
        (run(&resumed, &mut host), host)
    })
    .join()
    .unwrap();
    assert_eq!(number(outcome), 18.0);
    assert_eq!(host.reports, [4.0]);
}

/// Synthetic global-identifier fixtures, not light-cljs output. Run the
/// existing bounded property harness with collection at every instruction,
/// while parked, and after resuming on another worker.
#[test]
fn global_accessors_wait_collect_and_migrate_without_replay() {
    for (src, expected, reports) in [
        (
            "Object.defineProperty(globalThis, 'x', {get() { __rt.effect(2, 1); return __rt.need(5, 3) + this.offset; }}); globalThis.offset = 2; x",
            9.0,
            vec![1.0],
        ),
        (
            "Object.defineProperty(globalThis, 'x', {set(v) { __rt.effect(2, 1); this.saved = __rt.need(5, 3) + v.n; return 99; }}); let v = (x = {n: 2}); saved + v.n",
            11.0,
            vec![1.0],
        ),
        (
            "Object.defineProperty(globalThis, 'x', {get() { __rt.effect(2, 1); __rt.need(5, 3); return 9; }}); typeof x === 'number' ? 1 : 0",
            1.0,
            vec![1.0],
        ),
        (
            "globalThis.saved = 2; Object.defineProperty(globalThis, 'x', {get() { __rt.effect(2, 1); return __rt.need(5, 3) + this.saved; }, set(v) { __rt.effect(2, 2); this.saved = v; }}); x += 1; saved",
            10.0,
            vec![1.0, 2.0],
        ),
        (
            "globalThis.saved = 2; Object.defineProperty(globalThis, 'x', {get() { __rt.effect(2, 1); return this.saved; }, set(v) { __rt.effect(2, 2); this.saved = __rt.need(5, 3) + v; }}); let old = x++; saved + old",
            12.0,
            vec![1.0, 2.0],
        ),
        (
            "let p = {get x() { __rt.effect(2, 1); return __rt.need(5, 3) + this.offset; }}; Object.setPrototypeOf(globalThis, p); globalThis.offset = 2; x",
            9.0,
            vec![1.0],
        ),
        (
            "Object.defineProperty(globalThis, 'x', {get() { __rt.effect(2, 1); __rt.need(5, 3); throw 9; }}); let n = 0; try { x; } catch(e) { n = e; } n",
            9.0,
            vec![1.0],
        ),
    ] {
        let (result, host) = resume_property_fixture(src);
        assert_eq!(result, expected, "{src}");
        assert_eq!(host.reports, reports, "no replay: {src}");
    }
}

/// Synthetic fixtures, not light-cljs output. Every callback must resume
/// once, retaining the receiver and completed string parts across GC/migration.
#[test]
fn array_join_resumes_without_repeating_conversions() {
    for src in [
        "let n = 0; let s = [{toString() { n++; __rt.effect(1, 3); return 'x' + __rt.need(5, 3); }}, {toString() { n++; return 'y'; }}].join('-'); s === 'x7-y' && n === 2",
        "let n = 0; let o = {get length() { n++; __rt.effect(1, 3); return __rt.need(5, 3) - 5; }, 0: 'x', 1: 'y'}; Array.prototype.join.call(o) === 'x,y' && n === 1",
        // The inline separator has no local/root other than join.separator
        // while the length getter waits, before separator coercion begins.
        "let n = 0; let o = {get length() { __rt.effect(1, 3); return __rt.need(5, 3) - 5; }, 0: 'x', 1: 'y'}; Array.prototype.join.call(o, {toString() { n++; return '-'; }}) === 'x-y' && n === 1",
        "let n = 0; let o = {length: {valueOf() { n++; __rt.effect(1, 3); return __rt.need(5, 3) - 5; }}, 0: 'x', 1: 'y'}; Array.prototype.join.call(o) === 'x,y' && n === 1",
        "let n = 0; let s = {toString() { n++; __rt.effect(1, 3); __rt.need(5, 3); return '-'; }}; [1, 2].join(s) === '1-2' && n === 1",
        "let n = 0; let a = [1, 2]; Object.defineProperty(a, 1, {get() { n++; __rt.effect(1, 3); return __rt.need(5, 3); }}); a.join() === '1,7' && n === 1",
        "let n = 0; let a = [1, 2]; Object.defineProperty(a, 'join', {get() { n++; __rt.effect(1, 3); __rt.need(5, 3); return function () { return this[1]; }; }}); a.toString() === 2 && n === 1",
        "let n = 0; let a = {get join() { n++; return function () { n++; __rt.effect(1, 3); return this.x + __rt.need(5, 3); }; }, x: 2}; Array.prototype.toString.call(a) === 9 && n === 2",
    ] {
        let rt = Runtime::new();
        let code = compile_script("array-join-wait", src).unwrap();
        let task = rt.spawn(&code, &[]).unwrap();
        let mut host = Recorder::default();
        let expected = key(&rt, NUMBER);
        let mut slices = 0;
        loop {
            slices += 1;
            assert!(slices < 2000, "{src}");
            match task.run_with(Budget::instructions(1), &mut host).unwrap() {
                RunOutcome::Yielded => {
                    rt.collect().unwrap();
                }
                outcome => {
                    assert_eq!(waiting(outcome), expected);
                    break;
                }
            }
        }
        assert_eq!(host.effects, [3.0]);
        let request = one_request(&task, expected);
        rt.collect().unwrap();
        let resumed = only_runnable(publish_number(&rt, &request, 7.0));
        rt.collect().unwrap();
        let (result, host) = thread::spawn(move || {
            let mut host = host;
            (run(&resumed, &mut host), host)
        })
        .join()
        .unwrap();
        let RunOutcome::Complete(value) = result else {
            panic!("unexpected {result:?}: {src}");
        };
        assert_eq!(value.view(), ValueView::Bool(true), "{src}");
        assert_eq!(host.effects, [3.0], "no replay: {src}");
        assert_eq!(task.dependency_log().edges(), &[expected]);
    }
}

/// Synthetic fixtures, not light-cljs output. All observable calls in
/// Array.from resume once, even when collected at every instruction and
/// resumed on another thread. The result prefix is rooted by the native.
#[test]
fn array_from_iterator_waits_on_dependency() {
    for src in [
        "let n = 0, o = {next() { n++; __rt.effect(1, 3); let v = __rt.need(5, 3); return {value: n + v, done: n > 2}; }}; o[Symbol.iterator] = function() { return this; }; let a = Array.from(o); a.join() === '8,9' && n === 3",
        "let n = 0, o = {}; Object.defineProperty(o, Symbol.iterator, {get() { n++; __rt.effect(1, 3); __rt.need(5, 3); return function() { return [1, 2].values(); }; }}); Array.from(o).join() === '1,2' && n === 1",
        "let n = 0, o = {}; o[Symbol.iterator] = function() { n++; __rt.effect(1, 3); __rt.need(5, 3); return [1, 2].values(); }; Array.from(o).join() === '1,2' && n === 1",
        "let n = 0, o = {get next() { n++; __rt.effect(1, 3); __rt.need(5, 3); return function() { return {done: true}; }; }}; o[Symbol.iterator] = function() { return this; }; Array.from(o).length === 0 && n === 1",
        "let n = 0; let a = Array.from([{x: 1}, {x: 2}], function(v, i) { n++; if (i === 1) { __rt.effect(1, 3); __rt.need(5, 3); } return {x: this.x + v.x}; }, {x: 4}); a[0].x === 5 && a[1].x === 6 && n === 2",
        "let n = 0; let a = Array.from({length: 2, 0: {x: 1}, 1: {x: 2}}, function(v, i) { n++; if (i === 1) { __rt.effect(1, 3); __rt.need(5, 3); } return {x: this.x + v.x}; }, {x: 4}); a[0].x === 5 && a[1].x === 6 && n === 2",
        "let n = 0, o = {get length() { n++; __rt.effect(1, 3); return __rt.need(5, 3) - 5; }, 0: 'a', 1: 'b'}; Array.from(o).join() === 'a,b' && n === 1",
        "let n = 0, o = {length: {valueOf() { n++; __rt.effect(1, 3); return __rt.need(5, 3) - 5; }}, 0: 'a', 1: 'b'}; Array.from(o).join() === 'a,b' && n === 1",
        "let n = 0, o = {length: 2, 0: {x: 1}, get 1() { n++; __rt.effect(1, 3); return {x: __rt.need(5, 3)}; }}; let a = Array.from(o); a[0].x === 1 && a[1].x === 7 && n === 1",
        "let n = 0, c = 0, o = {next() { return {value: ++n}; }, return() { c++; __rt.effect(1, 3); __rt.need(5, 3); throw 99; }}; o[Symbol.iterator] = function() { return this; }; let ok = false; try { Array.from(o, function(v) { if (v === 2) throw 17; return v; }); } catch(e) { ok = e === 17; } ok && c === 1 && n === 2",
        "let c = 0, o = {next() { return {value: 1}; }, get return() { c++; __rt.effect(1, 3); __rt.need(5, 3); return function() { return {}; }; }}; o[Symbol.iterator] = function() { return this; }; let ok = false; try { Array.from(o, function() { throw {x: 17}; }); } catch(e) { ok = e.x === 17; } ok && c === 1",
        "let n = 0, o = {next() { return {get done() { n++; if (n === 1) { __rt.effect(1, 3); __rt.need(5, 3); } return n > 1; }, value: 1}; }}; o[Symbol.iterator] = function() { return this; }; Array.from(o)[0] === 1 && n === 2",
        "let n = 0, o = {next() { return ++n === 1 ? {get value() { __rt.effect(1, 3); return __rt.need(5, 3); }} : {done: true}; }}; o[Symbol.iterator] = function() { return this; }; Array.from(o)[0] === 7 && n === 2",
    ] {
        let rt = Runtime::new();
        let code = compile_script("array-from-wait", src).unwrap();
        let task = rt.spawn(&code, &[]).unwrap();
        let mut host = Recorder::default();
        let expected = key(&rt, NUMBER);
        let mut slices = 0;
        loop {
            slices += 1;
            assert!(slices < 3000, "{src}");
            match task.run_with(Budget::instructions(1), &mut host).unwrap() {
                RunOutcome::Yielded => {
                    rt.collect().unwrap();
                }
                outcome => {
                    assert_eq!(waiting(outcome), expected);
                    break;
                }
            }
        }
        let request = one_request(&task, expected);
        rt.collect().unwrap();
        let resumed = only_runnable(publish_number(&rt, &request, 7.0));
        rt.collect().unwrap();
        let worker_rt = rt.clone();
        let (result, host) = thread::spawn(move || {
            let mut host = host;
            for _ in 0..3000 {
                match resumed
                    .run_with(Budget::instructions(1), &mut host)
                    .unwrap()
                {
                    RunOutcome::Yielded => {
                        worker_rt.collect().unwrap();
                    }
                    outcome => return (outcome, host),
                }
            }
            panic!("Array.from did not complete");
        })
        .join()
        .unwrap();
        let RunOutcome::Complete(value) = result else {
            panic!("unexpected {result:?}: {src}");
        };
        assert_eq!(value.view(), ValueView::Bool(true), "{src}");
        let effects = if src.contains("let v = __rt.need") {
            vec![3.0; 3]
        } else {
            vec![3.0]
        };
        assert_eq!(host.effects, effects, "no replay: {src}");
        assert_eq!(task.dependency_log().edges(), &[expected]);
    }
}

/// Synthetic fixture, not light-cljs output. Dependency failure is an
/// exception from the mapper; consumer cancellation discards the native
/// boundary and executes only the callback's ordinary finally cleanup.
#[test]
fn array_from_mapper_failure_and_cancellation() {
    let src = "let o = {next() { return {value: 1}; }, return() { __rt.effect(2, 5); return {}; }}; o[Symbol.iterator] = function() { return this; }; let caught = 0; try { Array.from(o, function() { try { __rt.need(5, 3); } finally { __rt.effect(2, 9); } }); } catch(e) { caught = 17; } caught";
    let rt = Runtime::new();
    let exe = compile_script("array-from-failure", src).unwrap();
    let task = rt.spawn(&exe, &[]).unwrap();
    let mut host = Recorder::default();
    let expected = key(&rt, NUMBER);
    assert_eq!(waiting(run(&task, &mut host)), expected);
    let request = one_request(&task, expected);
    let error = rt.root(Primitive::Number(42.0));
    let wakeups = rt
        .complete(
            request.key(),
            request.producer(),
            Completion::Failed(&error),
        )
        .unwrap();
    rt.collect().unwrap();
    assert_eq!(number(run(&only_runnable(wakeups), &mut host)), 17.0);
    assert_eq!(host.reports, [9.0, 5.0]);

    let rt = Runtime::new();
    let task = rt.spawn(&exe, &[]).unwrap();
    let mut host = Recorder::default();
    let expected = key(&rt, NUMBER);
    assert_eq!(waiting(run(&task, &mut host)), expected);
    let request = one_request(&task, expected);
    let Cancel::Runnable(cancelled) = task.cancel() else {
        panic!("runnable cancellation");
    };
    assert!(matches!(run(&cancelled, &mut host), RunOutcome::Cancelled));
    assert_eq!(host.reports, [9.0]);
    rt.collect().unwrap();
    assert!(
        publish_number(&rt, &request, 7.0)
            .into_runnable()
            .is_empty()
    );
}

/// Synthetic fixtures, not light-cljs output. Compare a suspended sort to
/// an uninterrupted one, collecting at every step and migrating after wakeup.
#[test]
fn array_sort_comparator_waits_on_dependency() {
    for src in [
        // Number is itself a resumable native comparator; its object
        // conversion waits while a second native (sort) remains pending.
        "let calls = 0; function obj(i) { return {i: i, valueOf() { __rt.effect(1, ++calls); if (calls === 2) __rt.need(5,3); return 0; }}; } let a = [obj(4),obj(1),obj(3),obj(2)]; a.sort(Number); a[0].i === 4 && a[1].i === 1 && a[2].i === 3 && a[3].i === 2",
        "let calls = 0, a = [{k:4}, {k:1}, {k:3}, {k:2}, {k:1}]; a.sort(function(a,b) { __rt.effect(1, ++calls); if (calls === 2) __rt.need(5,3); return a.k-b.k; }); a[0].k === 1 && a[1].k === 1 && a[2].k === 2 && a[3].k === 3 && a[4].k === 4",
        // Only the snapshot retains the objects after the first callback.
        "let calls = 0, a = [{k:4}, {k:1}, {k:3}, {k:2}]; a.sort(function(x,y) { __rt.effect(1, ++calls); if (calls === 1) { delete a[0]; delete a[1]; delete a[2]; delete a[3]; } if (calls === 2) __rt.need(5,3); return x.k-y.k; }); a[0].k === 1 && a[1].k === 2 && a[2].k === 3 && a[3].k === 4",
        "let calls = 0; let a = [4,1,3,2]; a.sort(function(x,y) { return {valueOf() { __rt.effect(1, ++calls); if (calls === 2) __rt.need(5,3); return x-y; }}; }); a.join() === '1,2,3,4'",
        "let calls = 0; function obj(k) { return {k: k, toString() { __rt.effect(1, ++calls); if (calls === 2) __rt.need(5,3); return this.k; }}; } let a = [obj('d'),obj('a'),obj('c'),obj('b')]; a.sort(); a[0].k === 'a' && a[1].k === 'b' && a[2].k === 'c' && a[3].k === 'd'",
        "let calls = 0, o = {get length() { __rt.effect(1, ++calls); __rt.need(5,3); return 2; }, 0: 2, 1: 1}; Array.prototype.sort.call(o); o[0] === 1 && o[1] === 2",
        "let calls = 0, o = {length: 2, get 0() { __rt.effect(1, ++calls); __rt.need(5,3); return 2; }, set 0(v) { __rt.effect(1, v); __rt.need(5,3); }, 1: 1}; Array.prototype.sort.call(o); o[1] === 2",
    ] {
        let mut baseline = None;
        for wait in [false, true] {
            let rt = Runtime::new();
            let source = if wait {
                src.to_string()
            } else {
                src.replace("__rt.need(5,3)", "7")
            };
            let task = rt
                .spawn(&compile_script("sort-wait", &source).unwrap(), &[])
                .unwrap();
            let mut host = Recorder::default();
            let expected = key(&rt, NUMBER);
            let mut completed = None;
            for steps in 0..4000 {
                match task.run_with(Budget::instructions(1), &mut host).unwrap() {
                    RunOutcome::Yielded => {
                        rt.collect().unwrap();
                    }
                    RunOutcome::Complete(value) if !wait => {
                        completed = Some(value);
                        break;
                    }
                    outcome => {
                        assert!(wait);
                        assert_eq!(waiting(outcome), expected);
                        break;
                    }
                }
                assert!(steps < 3999, "sort did not reach wait/completion");
            }
            if wait {
                let request = one_request(&task, expected);
                rt.collect().unwrap();
                let resumed = only_runnable(publish_number(&rt, &request, 7.0));
                let worker_rt = rt.clone();
                let (value, resumed_host) = thread::spawn(move || {
                    for _ in 0..4000 {
                        match resumed
                            .run_with(Budget::instructions(1), &mut host)
                            .unwrap()
                        {
                            RunOutcome::Yielded => {
                                worker_rt.collect().unwrap();
                            }
                            RunOutcome::Complete(value) => return (value, host),
                            other => panic!("unexpected resumed sort: {other:?}"),
                        }
                    }
                    panic!("sort did not complete");
                })
                .join()
                .unwrap();
                completed = Some(value);
                host = resumed_host;
                assert_eq!(
                    host.effects,
                    *baseline.as_ref().unwrap(),
                    "completed calls replayed: {src}"
                );
                assert_eq!(task.dependency_log().edges(), &[expected]);
            } else {
                baseline = Some(host.effects);
            }
            assert_eq!(completed.unwrap().view(), ValueView::Bool(true), "{src}");
        }
    }
}

/// Synthetic fixture, not light-cljs output. Dependency failures unwind the
/// comparator; cancellation drops the sort and runs the callback's finally.
#[test]
fn array_sort_comparator_failure_and_cancellation() {
    let source = "let a = [2,1]; let caught = false; try { a.sort(function(x,y) { try { __rt.need(5,3); } finally { __rt.effect(2,9); } return x-y; }); } catch(e) { caught = true; } caught && a.join() === '2,1'";
    let code = compile_script("sort-failure", source).unwrap();
    for cancel in [false, true] {
        let rt = Runtime::new();
        let task = rt.spawn(&code, &[]).unwrap();
        let mut host = Recorder::default();
        let expected = key(&rt, NUMBER);
        assert_eq!(waiting(run(&task, &mut host)), expected);
        let request = one_request(&task, expected);
        rt.collect().unwrap();
        if cancel {
            let Cancel::Runnable(cancelled) = task.cancel() else {
                panic!("runnable cancellation");
            };
            assert!(matches!(run(&cancelled, &mut host), RunOutcome::Cancelled));
            assert!(
                publish_number(&rt, &request, 7.0)
                    .into_runnable()
                    .is_empty()
            );
        } else {
            let error = rt.root(Primitive::Number(42.0));
            let wakeups = rt
                .complete(
                    request.key(),
                    request.producer(),
                    Completion::Failed(&error),
                )
                .unwrap();
            let RunOutcome::Complete(value) = run(&only_runnable(wakeups), &mut host) else {
                panic!("sort did not catch failure");
            };
            assert_eq!(value.view(), ValueView::Bool(true));
        }
        assert_eq!(host.reports, [9.0]);
        rt.collect().unwrap();
    }
}
