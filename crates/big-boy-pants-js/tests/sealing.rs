//! Publishing state a boot task leaves behind (light-cljs-646;
//! `crate::vm::seal`, `crate::vm::function`, "Fixed and sealed bindings"):
//! bindings that are never assigned, bindings sealed after load-time
//! reassignment, and registries and prototypes sealed in place. Each value
//! is booted in one task, published, and then used by tasks on other
//! threads. Synthetic fixtures, not light-cljs output.

use std::thread;

use big_boy_pants_js::frontend::compile_function;
use big_boy_pants_js::{PublishError, Rooted, RunOutcome, Runtime, ValueView};

fn run(rt: &Runtime, source: &str, args: &[&Rooted]) -> Result<Rooted, String> {
    let f = compile_function(source).map_err(|e| e.to_string())?;
    match rt.spawn(&f, args).unwrap().run() {
        Ok(RunOutcome::Complete(v)) => Ok(v),
        other => Err(format!("{other:?}")),
    }
}

fn boot(rt: &Runtime, body: &str) -> Rooted {
    run(rt, &format!("function boot() {{ {body} }}"), &[]).unwrap()
}

/// Call `f(arg)` in a new task on each of two threads; their results.
fn call_on_two_threads(rt: &Runtime, f: &Rooted, arg: f64) -> [ValueView; 2] {
    let call = compile_function("function call(f, x) { return f(x); }").unwrap();
    let x = rt.root(big_boy_pants_js::Primitive::Number(arg));
    let tasks = [
        rt.spawn(&call, &[f, &x]).unwrap(),
        rt.spawn(&call, &[f, &x]).unwrap(),
    ];
    thread::scope(|s| {
        let handles = tasks
            .iter()
            .map(|task| {
                s.spawn(move || match task.run().unwrap() {
                    RunOutcome::Complete(v) => v.view(),
                    other => panic!("{other:?}"),
                })
            })
            .collect::<Vec<_>>();
        [0, 1]
            .map(|_| ValueView::Undefined)
            .into_iter()
            .zip(handles)
            .map(|(_, h)| h.join().unwrap())
            .collect::<Vec<_>>()
            .try_into()
            .unwrap()
    })
}

fn rejection(rt: &Runtime, value: &Rooted) -> String {
    match rt.publish(value) {
        Err(e @ PublishError::NotShareable { .. }) => e.to_string(),
        other => panic!("expected a rejection, got {other:?}"),
    }
}

/// A binding a closure captures before its initialization (a function
/// declaration called by an earlier one, a later `const`) lives in a cell.
/// When nothing assigns it, the cell is fixed once initialized: the closure
/// publishes, and tasks on other threads read the binding.
#[test]
fn bindings_that_are_never_assigned_publish_once_initialized() {
    let rt = Runtime::new();
    for body in [
        "function f(x) { return g(x); } function g(x) { return x * 2; } return f;",
        "const f = x => g(x); const g = x => x * 2; return f;",
        "const f = x => x <= 0 ? 0 : 2 + h(x - 1); const h = x => f(x); return f;",
    ] {
        let f = boot(&rt, body);
        rt.publish(&f).unwrap_or_else(|e| panic!("{body}: {e}"));
        assert_eq!(
            call_on_two_threads(&rt, &f, 21.0),
            [ValueView::Number(42.0), ValueView::Number(42.0)],
            "{body}"
        );
    }
    // Published before the capture is initialized: rejected at the cell.
    let early = boot(&rt, "const f = () => g; if (true) return f; const g = 1;");
    assert!(
        rejection(&rt, &early)
            .contains("value.capture(0) is a task-owned uninitialized binding cell"),
        "{}",
        rejection(&rt, &early)
    );
}

/// A binding its own function reassigns while loading is sealable: it is
/// rejected until `__rt.sealBindings` seals it, then publishes, and a later
/// assignment throws. One a nested function assigns (a counter) stays
/// mutable and task-owned; `sealBindings` passes over it.
#[test]
fn seal_bindings_seals_load_time_reassignments_only() {
    let rt = Runtime::new();
    let source = "function seq(x) { return x; } \
         seq = (inner => x => inner(x) + 1)(seq); \
         let n = 0; const count = () => ++n; \
         const f = x => seq(x) + count() * 0; const g = x => seq(x); \
         return __rt.formVector(f, g, SEAL);";
    let parts = |booted: &Rooted| {
        rt.scope(|s| {
            let v = s.local(booted).unwrap();
            [0, 1, 2].map(|i| s.persist(s.form_nth(v, i).unwrap()))
        })
    };
    let [_, g, _] = parts(&boot(&rt, &source.replace("SEAL", "0")));
    let error = rejection(&rt, &g);
    assert!(
        error.contains("value.capture(0) is a task-owned unsealed binding cell"),
        "{error}"
    );
    // Sealing is the boot task's: another task seals none of its cells.
    let other = run(&rt, "function s(v) { return __rt.sealBindings(v); }", &[&g]).unwrap();
    assert_eq!(other.view(), ValueView::Number(0.0));
    assert!(rt.publish(&g).is_err());
    let [f, g, sealed] = parts(&boot(
        &rt,
        &source.replace("SEAL", "__rt.sealBindings(__rt.formVector(f, g))"),
    ));
    assert_eq!(sealed.view(), ValueView::Number(1.0));
    rt.publish(&g).unwrap();
    assert_eq!(
        call_on_two_threads(&rt, &g, 41.0),
        [ValueView::Number(42.0), ValueView::Number(42.0)]
    );
    // `n` is assigned by `count`: still a task-owned cell, so `f` is
    // rejected through `count`.
    let error = rejection(&rt, &f);
    assert!(error.contains("is a task-owned binding cell"), "{error}");

    // After the seal, the declaring function's own assignment throws.
    let out = boot(
        &rt,
        "let g = 1; const f = () => g; g = 2; const k = __rt.sealBindings(f); \
         try { g = 3; return 'assigned'; } \
         catch (e) { return [k, e instanceof TypeError, f()].join(' '); }",
    );
    assert_eq!(out.string().unwrap().to_string_lossy(), "1 true 2");
}

/// `__rt.seal` seals registries and prototypes in place: an object of hook
/// functions, an array holding it, and a constructor with its prototype.
/// The closure over them publishes, other tasks call it and construct
/// instances, and writes to a sealed object throw in every task.
#[test]
fn seal_shares_registries_and_prototypes_by_identity() {
    let rt = Runtime::new();
    let source = "const hooks = {}; hooks.inc = x => x + 1; \
         const keys = []; keys.push(hooks); \
         function Point(x) { this.x = x; } \
         Point.prototype.norm = function () { return this.x; }; \
         const use = x => keys[0].inc(new Point(x).norm()); \
         SEAL return __rt.formVector(use, hooks, keys, Point);";
    let unsealed = boot(&rt, &source.replace("SEAL", ""));
    let error = rejection(&rt, &unsealed);
    assert!(error.contains("is a task-owned"), "{error}");
    let booted = boot(
        &rt,
        &source.replace(
            "SEAL",
            "if (__rt.seal(hooks) !== hooks) throw 'identity'; __rt.seal(keys); \
             __rt.seal(Point.prototype); __rt.seal(Point);",
        ),
    );
    rt.publish(&booted).unwrap();
    let use_ = rt.scope(|s| {
        let v = s.local(&booted).unwrap();
        s.persist(s.form_nth(v, 0).unwrap())
    });
    assert_eq!(
        call_on_two_threads(&rt, &use_, 41.0),
        [ValueView::Number(42.0), ValueView::Number(42.0)]
    );
    // Reads work in another task; writes throw there and in the owner.
    let probe = "function probe(v) { \
           const [use, hooks, keys, Point] = [0, 1, 2, 3].map(i => __rt.formNth(v, i)); \
           const out = [Object.isFrozen(hooks), keys.length, typeof Point.prototype.norm]; \
           for (const write of [() => { hooks.y = 1; }, () => { keys.push(1); }, \
                                () => { Point.prototype.norm = null; }, () => { Point.z = 1; }, \
                                () => { delete hooks.inc; }]) { \
             try { write(); out.push('wrote'); } catch (e) { out.push(e instanceof TypeError); } \
           } \
           return out.join(' '); }";
    let result = run(&rt, probe, &[&booted]).unwrap();
    assert_eq!(
        result.string().unwrap().to_string_lossy(),
        "true 1 function true true true true true"
    );
    // A Map does not seal.
    let map = run(
        &rt,
        "function m() { try { __rt.seal(new Map()); return 'sealed'; } \
         catch (e) { return String(e instanceof TypeError); } }",
        &[],
    )
    .unwrap();
    assert_eq!(map.string().unwrap().to_string_lossy(), "true");
}

/// A class's own constants (its inner name, private names) are fixed
/// cells, so its static methods publish once the class and its prototype
/// are sealed; sealing a concise method seals its home object too.
#[test]
fn classes_and_concise_methods_publish_once_sealed() {
    let rt = Runtime::new();
    let source = "class Store { #s = 1; static has(v) { return #s in v; } \
                    static make() { return new Store(); } } \
         const hooks = { inc(x) { return x + 1; } }; \
         SEAL return x => Store.has(Store.make()) && !Store.has(hooks) ? hooks.inc(x) : -1;";
    let unsealed = boot(&rt, &source.replace("SEAL", ""));
    let error = rejection(&rt, &unsealed);
    assert!(error.contains("is a task-owned object"), "{error}");
    let f = boot(
        &rt,
        &source.replace(
            "SEAL",
            "__rt.seal(Store.prototype); __rt.seal(Store); __rt.seal(Store.has); \
             __rt.seal(Store.make); __rt.seal(hooks.inc);",
        ),
    );
    rt.publish(&f).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        call_on_two_threads(&rt, &f, 41.0),
        [ValueView::Number(42.0), ValueView::Number(42.0)]
    );
}

/// A clone made by `fnApplyNull` or `fnWithApplyTo` keeps its original's
/// property record. Once the original is published, that record is known
/// shareable, and publishing the clone must accept it instead of taking it
/// for a record attached during the walk (which retried forever).
#[test]
fn publishing_clones_with_already_shared_property_records_terminates() {
    let rt = std::sync::Arc::new(Runtime::new());
    let booted = boot(
        &rt,
        "const f = () => 1; f.x = 1; __rt.seal(f); \
         return __rt.formVector(f, __rt.fnApplyNull(f), __rt.fnWithApplyTo(f, xs => 2));",
    );
    let [f, null_clone, apply_clone] = rt.scope(|s| {
        let v = s.local(&booted).unwrap();
        [0, 1, 2].map(|i| s.persist(s.form_nth(v, i).unwrap()))
    });
    rt.publish(&f).unwrap();
    for clone in [null_clone, apply_clone] {
        let (tx, rx) = std::sync::mpsc::channel();
        let rt = rt.clone();
        thread::spawn(move || {
            tx.send(rt.publish(&clone).map_err(|e| e.to_string()))
                .unwrap()
        });
        rx.recv_timeout(std::time::Duration::from_secs(30))
            .expect("publication of the clone did not terminate")
            .unwrap();
    }
}

/// A sealed array rejects every change with a `TypeError`, in the owner
/// and in other tasks, in sloppy and strict code: indexed, named and
/// `length` writes, deletion, `push` and `Object.defineProperty`.
#[test]
fn sealed_array_mutations_throw_in_sloppy_and_strict_tasks() {
    let rt = Runtime::new();
    let writes = "[a => { a[0] = 2; }, a => { a[5] = 2; }, a => { a.x = 1; }, \
                  a => { a.length = 0; }, a => delete a[0], a => a.push(3), \
                  a => Object.defineProperty(a, 0, {value: 9})]\
                  .map(w => { try { w(a); return 'changed'; } \
                              catch (e) { return e instanceof TypeError; } })\
                  .concat([a[0], a.length]).join(' ')";
    let expected = "true true true true true true true 1 1";
    for strict in ["", "'use strict';"] {
        let owner = boot(
            &rt,
            &format!("{strict} const a = __rt.seal([1]); return {writes};"),
        );
        assert_eq!(
            owner.string().unwrap().to_string_lossy(),
            expected,
            "{strict}"
        );
        let a = boot(&rt, "return __rt.seal([1]);");
        rt.publish(&a).unwrap();
        let foreign = run(
            &rt,
            &format!("function probe(a) {{ {strict} return {writes}; }}"),
            &[&a],
        )
        .unwrap();
        assert_eq!(
            foreign.string().unwrap().to_string_lossy(),
            expected,
            "{strict}"
        );
    }
}

/// Sealing is shallow: a sealed object, array, function record, sealed
/// binding or fixed binding that still holds task-owned state is rejected
/// by publication and by an atom install, at the unsealed object; sealing
/// that object too makes the graph publish.
#[test]
fn shallow_seals_preserve_transitive_publication_checks() {
    let rt = Runtime::new();
    for (holder, rest) in [
        (
            "const child = {}; const v = __rt.seal({child});",
            "__rt.seal(child);",
        ),
        (
            "const child = {}; const v = __rt.seal([child]);",
            "__rt.seal(child);",
        ),
        (
            "const child = {}; const v = () => 1; v.child = child; __rt.seal(v);",
            "__rt.seal(child);",
        ),
        (
            "const child = {}; let c = 0; c = child; const v = () => c; __rt.sealBindings(v);",
            "__rt.seal(child);",
        ),
        (
            "const v = () => child; const child = {};",
            "__rt.seal(child);",
        ),
    ] {
        let install = |v: &Rooted| {
            run(
                &rt,
                "function install(v) { return __rt.atomLoad(__rt.atomNew(v)) === v; }",
                &[v],
            )
        };
        let v = boot(&rt, &format!("{holder} return v;"));
        let error = rejection(&rt, &v);
        assert!(
            error.contains("is a task-owned object"),
            "{holder}: {error}"
        );
        let refused = install(&v).unwrap_err();
        assert!(refused.contains("cannot publish"), "{holder}: {refused}");
        let v = boot(&rt, &format!("{holder} {rest} return v;"));
        rt.publish(&v).unwrap_or_else(|e| panic!("{holder}: {e}"));
        assert_eq!(
            install(&v).unwrap().view(),
            ValueView::Bool(true),
            "{holder}"
        );
    }
}
