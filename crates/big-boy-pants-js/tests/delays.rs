//! `delay` values (light-cljs-613, `src/collections/lazy.rs`): the same
//! once-realized cell a lazy sequence has, whose value is kept as the body
//! returned it. One task runs a shared delay's body once while racing
//! readers on other tasks park; a failure reaches the readers of the failed
//! attempt and a later `cljForce` runs the body again; a delay publishes
//! when its body (before) or its value (after) does. Synthetic JavaScript,
//! driven by hand.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;

use big_boy_pants_js::frontend::compile_function;
use big_boy_pants_js::{
    Budget, CellStatus, Host, HostCall, LAZY_REALIZATION, Primitive, Rooted, RunOutcome, Runtime,
    Task,
};

/// Counts body runs (`__rt.effect(1, …)`), optionally holding the first one
/// at two barriers: "inside the body" and "may return".
#[derive(Clone, Default)]
struct Bodies {
    runs: Arc<AtomicUsize>,
    hold: Option<(Arc<Barrier>, Arc<Barrier>)>,
}

impl Host for Bodies {
    fn effect(&mut self, call: HostCall) -> Result<Primitive, String> {
        assert_eq!(call.op, 1);
        let run = self.runs.fetch_add(1, Ordering::SeqCst);
        if run == 0
            && let Some((inside, release)) = &self.hold
        {
            inside.wait();
            release.wait();
        }
        Ok(Primitive::Number(run as f64))
    }
}

/// One task's result for `body`, a function body over `__rt`.
fn eval(rt: &Runtime, body: &str) -> Rooted {
    let f = compile_function(&format!("function test() {{ {body} }}")).unwrap();
    match rt.spawn(&f, &[]).unwrap().run().unwrap() {
        RunOutcome::Complete(v) => v,
        other => panic!("{body}: {other:?}"),
    }
}

/// A published delay whose body counts its runs and returns the vector
/// `[1 2 3]`, or throws `"boom"` when `throws`.
fn shared_delay(rt: &Runtime, throws: bool) -> Rooted {
    let body = if throws {
        "throw 'boom';"
    } else {
        "return __rt.formVector(1, 2, 3);"
    };
    let make = compile_function(&format!(
        "function make() {{ return __rt.delay(() => {{ __rt.effect(1, 0); {body} }}, null); }}"
    ))
    .unwrap();
    let RunOutcome::Complete(delay) = rt.spawn(&make, &[]).unwrap().run().unwrap() else {
        panic!("maker failed");
    };
    rt.publish(&delay).unwrap();
    delay
}

/// A reader: the kind and count of the delay's value, or the caught failure.
fn reader(rt: &Runtime, delay: &Rooted) -> Task {
    let read = compile_function(
        "function read(d) {
           try {
             const v = __rt.cljForce(d);
             return __rt.formKind(v) + ' ' + __rt.cljCount(v);
           } catch (e) { return 'caught ' + e; }
         }",
    )
    .unwrap();
    rt.spawn(&read, &[delay]).unwrap()
}

fn text(outcome: RunOutcome) -> String {
    match outcome {
        RunOutcome::Complete(v) => v.string().unwrap().to_string_lossy(),
        other => panic!("expected completion, got {other:?}"),
    }
}

/// Run `task` on its own thread until it parks on a realization; returns
/// the wait's key.
fn park(task: Task, host: Bodies) -> big_boy_pants_js::DependencyKey {
    thread::spawn(
        move || match task.run_with(Budget::UNLIMITED, &mut host.clone()) {
            Ok(RunOutcome::Waiting(key)) => key,
            other => panic!("expected a realization wait, got {other:?}"),
        },
    )
    .join()
    .unwrap()
}

#[test]
fn racing_readers_park_and_the_body_runs_once() {
    let rt = Runtime::new();
    let delay = shared_delay(&rt, false);
    let (inside, release) = (Arc::new(Barrier::new(2)), Arc::new(Barrier::new(2)));
    let host = Bodies {
        runs: Arc::default(),
        hold: Some((inside.clone(), release.clone())),
    };
    let (a, b) = (reader(&rt, &delay), reader(&rt, &delay));
    // A claims the cell and is held inside the body on its own thread.
    let realizer = {
        let (a, mut host) = (a.clone(), host.clone());
        thread::spawn(move || a.run_with(Budget::UNLIMITED, &mut host).unwrap())
    };
    inside.wait();
    // B, on another thread, finds the delay being realized and parks.
    let key = park(b.clone(), host.clone());
    assert_eq!(key.kind(), LAZY_REALIZATION);
    assert!(matches!(rt.cell_status(key), CellStatus::Pending { .. }));
    assert_eq!(rt.lazy_realizer(key), Some(a.id()));
    release.wait();
    // The value is kept as the body returned it: a vector, not its seq.
    assert_eq!(text(realizer.join().unwrap()), "vector 3");
    let woken = rt.take_lazy_wakeups();
    assert_eq!(woken.iter().map(Task::id).collect::<Vec<_>>(), [b.id()]);
    assert_eq!(text(b.run().unwrap()), "vector 3");
    assert_eq!(text(reader(&rt, &delay).run().unwrap()), "vector 3");
    assert_eq!(host.runs.load(Ordering::SeqCst), 1);
}

#[test]
fn a_failure_reaches_parked_readers_and_a_later_force_runs_the_body_again() {
    let rt = Runtime::new();
    let delay = shared_delay(&rt, true);
    let (inside, release) = (Arc::new(Barrier::new(2)), Arc::new(Barrier::new(2)));
    let host = Bodies {
        runs: Arc::default(),
        hold: Some((inside.clone(), release.clone())),
    };
    let (a, b) = (reader(&rt, &delay), reader(&rt, &delay));
    let realizer = {
        let (a, mut host) = (a.clone(), host.clone());
        thread::spawn(move || a.run_with(Budget::UNLIMITED, &mut host).unwrap())
    };
    inside.wait();
    park(b.clone(), host.clone());
    release.wait();
    assert_eq!(text(realizer.join().unwrap()), "caught boom");
    assert_eq!(rt.take_lazy_wakeups().len(), 1);
    assert_eq!(text(b.run().unwrap()), "caught boom");
    assert_eq!(host.runs.load(Ordering::SeqCst), 1);
    // Not cached, as in ClojureScript: the next force runs it again.
    let later = reader(&rt, &delay)
        .run_with(Budget::UNLIMITED, &mut host.clone())
        .unwrap();
    assert_eq!(text(later), "caught boom");
    assert_eq!(host.runs.load(Ordering::SeqCst), 2);
}

#[test]
fn a_delay_keeps_its_value_compares_by_identity_and_is_not_a_sequence() {
    let rt = Runtime::new();
    let v = eval(
        &rt,
        "let runs = 0;
         const d = __rt.delay(() => { runs++; return null; }, null);
         const e = __rt.delay(() => __rt.formMap(1, 2), null);
         const out = [__rt.formKind(d), __rt.formReferenceType(d), __rt.cljRealized(d)];
         out.push(__rt.cljForce(d), __rt.cljForce(d), runs, __rt.cljRealized(d));
         out.push(__rt.formKind(__rt.cljForce(e)), __rt.cljForce(5), __rt.cljForce(null));
         out.push(__rt.formEquals(d, d), __rt.formEquals(d, e),
                  __rt.formHash(d) === __rt.formHash(d), __rt.cljTruth(d));
         try { __rt.cljSeq(d); out.push('seq'); } catch (err) { out.push(err instanceof TypeError); }
         try { __rt.cljCount(d); out.push('count'); } catch (err) { out.push(err instanceof TypeError); }
         return out.join(' ');",
    );
    assert_eq!(
        v.string().unwrap().to_string_lossy(),
        "delay delay false   1 true map 5  true false true true true true"
    );
}

/// A body that forces its own delay calls the body again, as ClojureScript
/// does (light-cljs-613 review): a guarded reentry terminates, the inner
/// run's value is the running task's answer (realized, in the printer's
/// state) until the outer run ends, and the outer run's value is kept. An
/// outer run that throws after an inner run returned leaves the delay
/// realized to the inner value. Unguarded, it recurses until the native
/// call depth limit, a catchable error, and stays unrealized.
#[test]
fn a_body_that_forces_its_own_delay_runs_again_as_in_clojurescript() {
    let rt = Runtime::new();
    let v = eval(
        &rt,
        "let n = 0, d, seen;
         d = __rt.delay(() => {
           const v = ++n;
           if (v > 1) return v;
           const inner = __rt.cljForce(d);
           seen = [inner, __rt.cljRealized(d), __rt.formNth(__rt.cljDelayState(d), 0),
                   __rt.formNth(__rt.cljDelayState(d), 1), __rt.cljForce(d), n].join(',');
           return inner + 10;
         }, 'seed');
         const pending = __rt.formNth(__rt.cljDelayState(d), 1);
         const out = [pending, __rt.cljForce(d), __rt.cljForce(d), n, __rt.cljRealized(d), seen];
         let m = 0, e;
         e = __rt.delay(() => { const v = ++m; if (v > 1) return v * 100; __rt.cljForce(e); throw 'outer'; }, null);
         try { __rt.cljForce(e); } catch (x) { out.push(x); }
         out.push(__rt.cljRealized(e), __rt.cljForce(e), m);
         let f; f = __rt.delay(() => __rt.cljForce(f), null);
         try { __rt.cljForce(f); out.push('returned'); } catch (x) { out.push(x instanceof Error); }
         out.push(__rt.cljRealized(f));
         return out.join(' ');",
    );
    assert_eq!(
        v.string().unwrap().to_string_lossy(),
        "seed 12 12 2 true 2,true,true,2,2,2 outer true 200 2 true false"
    );
}

#[test]
fn a_body_capturing_task_owned_state_does_not_publish_and_names_the_path() {
    let rt = Runtime::new();
    let delay = eval(
        &rt,
        "let n = 0; const d = __rt.delay(() => { n++; return null; }, null); n = 1; return d;",
    );
    let error = rt.publish(&delay).unwrap_err().to_string();
    assert!(error.contains("value.thunk.capture(0)"), "{error}");
    // Realized to a task-owned value: the path names what it realized.
    let realized = eval(
        &rt,
        "const d = __rt.delay(() => ({}), null); __rt.cljForce(d); return d;",
    );
    let error = rt.publish(&realized).unwrap_err().to_string();
    assert!(error.contains("value.realized"), "{error}");
    // Realized to a shareable value, it publishes whatever its body was.
    let ready = eval(
        &rt,
        "let n = 0; const d = __rt.delay(() => { n++; return __rt.formVector(n); }, null); __rt.cljForce(d); return d;",
    );
    rt.publish(&ready).unwrap();
}

#[test]
fn a_shared_delay_cannot_realize_a_task_owned_value() {
    let rt = Runtime::new();
    let delay = eval(&rt, "return __rt.delay(() => ({}), null);");
    rt.publish(&delay).unwrap();
    let read = compile_function(
        "function read(d) { try { __rt.cljForce(d); return 'read'; } catch (e) { return String(e) + ' ' + __rt.cljRealized(d); } }",
    )
    .unwrap();
    let RunOutcome::Complete(v) = rt.spawn(&read, &[&delay]).unwrap().run().unwrap() else {
        panic!("reader failed");
    };
    let message = v.string().unwrap().to_string_lossy();
    assert!(
        message.starts_with("TypeError: a shared delay realized a value it cannot share")
            && message.ends_with(" false"),
        "{message}"
    );
}

#[test]
fn collection_keeps_a_delay_s_body_and_value() {
    let rt = Runtime::new();
    let delay = eval(
        &rt,
        "const d = __rt.delay(() => __rt.formVector(__rt.formKeyword(null, 'kept')), null); return d;",
    );
    rt.collect().unwrap();
    let read = compile_function(
        "function read(d) { return __rt.cljRealized(d) + ' ' + __rt.formName(__rt.formNth(__rt.cljForce(d), 0)); }",
    )
    .unwrap();
    assert_eq!(
        text(rt.spawn(&read, &[&delay]).unwrap().run().unwrap()),
        "false kept"
    );
    rt.collect().unwrap();
    assert_eq!(
        text(rt.spawn(&read, &[&delay]).unwrap().run().unwrap()),
        "true kept"
    );
}

/// `(Delay. f v)` (light-cljs-613 review): a nil or false body is a delay
/// realized to `v` (a false one printing as pending, as the reference
/// does); otherwise `v` is the pending value, kept across a failed run,
/// which publication checks under `.seed`.
#[test]
fn an_explicit_delay_keeps_its_pending_value() {
    let rt = Runtime::new();
    let v = eval(
        &rt,
        "const state = (d) => __rt.formNth(__rt.cljDelayState(d), 0) + '/' + __rt.formNth(__rt.cljDelayState(d), 1);
         const f = __rt.delay(false, 7), u = __rt.delay(null, 8);
         let tries = 0;
         const t = __rt.delay(() => { tries++; throw 'no'; }, 'kept');
         const s = __rt.delay(() => 'forced', 'seed');
         const out = [__rt.cljRealized(f), __rt.cljForce(f), state(f), __rt.cljRealized(u), __rt.cljForce(u), state(u)];
         try { __rt.cljForce(t); } catch (e) { out.push(e); }
         out.push(__rt.cljRealized(t), state(t), tries, state(s), __rt.cljForce(s), state(s));
         return out.join(' ');",
    );
    assert_eq!(
        v.string().unwrap().to_string_lossy(),
        "true 7 false/7 true 8 true/8 no false false/kept 1 false/seed forced true/forced"
    );
    let seeded = eval(&rt, "return __rt.delay(() => 1, __rt.formVector({}));");
    let error = rt.publish(&seeded).unwrap_err().to_string();
    assert!(error.contains("value.seed[0]"), "{error}");
}
