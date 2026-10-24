//! Publication of values from one task to another through the embedding
//! API (pellet clj-quickjs-18): shareable graphs pass by identity, graphs
//! reaching task-owned mutation are rejected with the offending path.
//!
//! Form-like values are synthetic fixtures, not light-cljs output; numbers
//! stand in for symbols. Programs are hand-assembled bytecode: early VM
//! tests, not JavaScript source support.

use std::thread;

use clj_quickjs::frontend::{compile_function, compile_script};
use clj_quickjs::{
    Assembler, Executable, HandleError, KeyName, Local, Opcode, PathStep, Primitive, Provenance,
    PublishError, Rooted, RunOutcome, Runtime, Scope, TaskId, ValueView, VmErrorKind,
};

fn complete(outcome: RunOutcome) -> Rooted {
    match outcome {
        RunOutcome::Complete(v) => v,
        other => panic!("expected Complete, got {other:?}"),
    }
}

fn num(n: f64) -> Primitive {
    Primitive::Number(n)
}

fn program(params: u16, build: impl FnOnce(&mut Assembler)) -> Executable {
    let mut asm = Assembler::new();
    build(&mut asm);
    asm.finish("publication", params, 4).unwrap()
}

/// Run a parameterless program and return its result and task id.
fn produce(rt: &Runtime, build: impl FnOnce(&mut Assembler)) -> (Rooted, TaskId) {
    let task = rt.spawn(&program(0, build), &[]).unwrap();
    (complete(task.run().unwrap()), task.id())
}

fn not_shareable(err: PublishError) -> (Vec<PathStep>, &'static str, Option<TaskId>) {
    match err {
        PublishError::NotShareable { path, found, owner } => (path.steps().to_vec(), found, owner),
        other => panic!("expected NotShareable, got {other:?}"),
    }
}

fn key(n: f64) -> KeyName {
    KeyName::Primitive(num(n))
}

fn n<'s>(s: &Scope<'s>, x: f64) -> Local<'s> {
    s.primitive(num(x))
}

/// `function (form, i) { return [form, nth(form, i)]; }`
fn whole_and_part() -> Executable {
    program(2, |a| {
        a.get_local(0)
            .get_local(0)
            .get_local(1)
            .op(Opcode::Nth)
            .new_vector(2)
            .op(Opcode::Return);
    })
}

#[test]
fn a_graph_of_shareable_parts_is_published_and_received_by_identity() {
    let rt = Runtime::new();
    // `[let, [x 1], {0: +, 1: [x]}]` with occurrence leaves.
    let at = |column| Provenance {
        source: 1,
        line: 1,
        column,
    };
    let (form, bindings, body) = rt.scope(|s| {
        let x = s.new_occurrence(n(s, 200.0), at(7)).unwrap();
        let bindings = s.new_vector(&[x, n(s, 1.0)]).unwrap();
        let args = s.new_vector(&[x]).unwrap();
        let plus = s.new_occurrence(n(s, 300.0), at(12)).unwrap();
        let body = s.new_map(&[(n(s, 0.0), plus), (n(s, 1.0), args)]).unwrap();
        let head = s.new_occurrence(n(s, 100.0), at(2)).unwrap();
        let form = s.new_vector(&[head, bindings, body]).unwrap();
        (s.persist(form), s.persist(bindings), s.persist(body))
    });
    assert_eq!(rt.publish(&form), Ok(()));
    assert_eq!(rt.publish(&form), Ok(()), "publishing again is fine");

    // Two receiving tasks on two threads.
    let exe = whole_and_part();
    let (one, two) = (rt.root(num(1.0)), rt.root(num(2.0)));
    let tasks = [
        rt.spawn(&exe, &[&form, &one]).unwrap(),
        rt.spawn(&exe, &[&form, &two]).unwrap(),
    ];
    let results: Vec<Rooted> = thread::scope(|t| {
        let running: Vec<_> = tasks
            .iter()
            .map(|task| t.spawn(|| complete(task.run().unwrap())))
            .collect();
        running.into_iter().map(|h| h.join().unwrap()).collect()
    });
    rt.scope(|s| {
        let form = s.local(&form).unwrap();
        for (result, part) in results.iter().zip([&bindings, &body]) {
            let result = s.local(result).unwrap();
            let got = s.items(result).unwrap();
            assert!(s.same_object(got[0], form), "the same form, not a copy");
            assert!(s.same_object(got[1], s.local(part).unwrap()));
        }
    });

    // A value a task built from published parts is itself publishable and
    // reaches a third task with the same identities.
    assert_eq!(rt.publish(&results[0]), Ok(()));
    let zero = rt.root(num(0.0));
    let echoed = complete(
        rt.spawn(&exe, &[&results[0], &zero])
            .unwrap()
            .run()
            .unwrap(),
    );
    rt.scope(|s| {
        let got = s.items(s.local(&echoed).unwrap()).unwrap();
        assert!(s.same_object(got[0], s.local(&results[0]).unwrap()));
        assert!(s.same_object(got[1], s.local(&form).unwrap()));
    });
}

#[test]
fn a_frozen_builder_result_is_publishable() {
    let rt = Runtime::new();
    // `let b = vector_builder(); conj!(b, 1); conj!(b, [2, 3]); return persistent!(b);`
    let (frozen, _) = produce(&rt, |a| {
        a.op(Opcode::NewVectorBuilder)
            .constant(num(1.0))
            .op(Opcode::BuilderConj)
            .constant(num(2.0))
            .constant(num(3.0))
            .new_vector(2)
            .op(Opcode::BuilderConj)
            .op(Opcode::Freeze)
            .op(Opcode::Return);
    });
    assert_eq!(rt.publish(&frozen), Ok(()));
    let one = rt.root(num(1.0));
    let got = complete(
        rt.spawn(&whole_and_part(), &[&frozen, &one])
            .unwrap()
            .run()
            .unwrap(),
    );
    rt.scope(|s| {
        let got = s.items(s.local(&got).unwrap()).unwrap();
        let frozen = s.local(&frozen).unwrap();
        assert!(s.same_object(got[0], frozen));
        assert!(s.same_object(got[1], s.nth(frozen, 1).unwrap()));
    });
}

#[test]
fn a_nested_builder_is_rejected_with_its_path_and_owner() {
    let rt = Runtime::new();
    // `let b = vector_builder(); return [0, {7: [1, b]}];`
    let (value, owner) = produce(&rt, |a| {
        a.op(Opcode::NewVectorBuilder).set_local(0);
        a.constant(num(0.0)).constant(num(7.0));
        a.constant(num(1.0)).get_local(0).new_vector(2);
        a.new_map(1).new_vector(2).op(Opcode::Return);
    });
    let err = rt.publish(&value).unwrap_err();
    assert_eq!(
        err.to_string(),
        format!(
            "cannot publish: value[1]{{7}}[1] is a task-owned vector builder \
             (owned by {owner}), which is not shareable"
        )
    );
    assert_eq!(
        not_shareable(err),
        (
            vec![
                PathStep::Index(1),
                PathStep::MapValue(key(7.0)),
                PathStep::Index(1)
            ],
            "vector builder",
            Some(owner),
        )
    );

    // A frozen builder handle is still task-owned: only its freeze result
    // is persistent. Here the handle is used as a map key.
    let (value, owner) = produce(&rt, |a| {
        a.op(Opcode::NewMapBuilder).set_local(0);
        a.get_local(0).op(Opcode::Freeze).op(Opcode::Pop);
        a.get_local(0)
            .constant(num(1.0))
            .new_map(1)
            .op(Opcode::Return);
    });
    assert_eq!(
        not_shareable(rt.publish(&value).unwrap_err()),
        (
            vec![PathStep::MapKey(KeyName::Object("map builder"))],
            "map builder",
            Some(owner),
        )
    );
}

#[test]
fn a_nested_mutable_object_is_rejected_with_its_path() {
    let rt = Runtime::new();
    let (nested, as_key, alone) = rt.scope(|s| {
        let obj = s.new_object();
        let pair = s.new_vector(&[n(s, 2.0), obj]).unwrap();
        let map = s.new_map(&[(n(s, 1.0), pair)]).unwrap();
        let at = Provenance {
            source: 2,
            line: 3,
            column: 4,
        };
        let occ = s.new_occurrence(map, at).unwrap();
        let nested = s.new_vector(&[n(s, 0.0), occ]).unwrap();
        let as_key = s.new_map(&[(obj, n(s, 1.0))]).unwrap();
        (s.persist(nested), s.persist(as_key), s.persist(obj))
    });
    assert_eq!(
        not_shareable(rt.publish(&nested).unwrap_err()),
        (
            vec![
                PathStep::Index(1),
                PathStep::OccurrenceValue,
                PathStep::MapValue(key(1.0)),
                PathStep::Index(1),
            ],
            "object",
            None,
        )
    );
    assert_eq!(
        not_shareable(rt.publish(&as_key).unwrap_err()),
        (
            vec![PathStep::MapKey(KeyName::Object("object"))],
            "object",
            None
        )
    );
    let err = rt.publish(&alone).unwrap_err();
    assert_eq!(
        err.to_string(),
        "cannot publish: value is a task-owned object, which is not shareable"
    );
    assert_eq!(not_shareable(err), (vec![], "object", None));

    // Replacing the object yields a publishable version; the rejected one
    // stays rejected.
    let fixed = rt.scope(|s| {
        let nested = s.local(&nested).unwrap();
        let occ = s.nth(nested, 1).unwrap();
        let map = s.occurrence_value(occ).unwrap();
        let pair = s.get(map, n(s, 1.0)).unwrap().unwrap();
        let pair = s.assoc(pair, n(s, 1.0), n(s, 3.0)).unwrap();
        let map = s.assoc(map, n(s, 1.0), pair).unwrap();
        let occ = s.new_occurrence(map, s.provenance(occ).unwrap()).unwrap();
        s.persist(s.assoc(nested, n(s, 1.0), occ).unwrap())
    });
    assert_eq!(rt.publish(&fixed), Ok(()));
    assert!(rt.publish(&nested).is_err());
}

#[test]
fn deep_and_heavily_shared_graphs_are_checked_without_stack_overflow() {
    const DEPTH: usize = 100_000;
    // A small stack: nothing in the check may recurse per level.
    thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(|| {
            let rt = Runtime::new();
            let (good, bad, shared) = rt.scope(|s| {
                let obj = s.new_object();
                // An empty vector not yet known shareable (popped from one
                // holding an object), so the check must walk every level.
                let holder = s.new_vector(&[obj]).unwrap();
                let base = s.pop(holder).unwrap();
                let nest = |bottom| (0..DEPTH).fold(bottom, |v, _| s.new_vector(&[v]).unwrap());
                let good = nest(base);
                let bad = nest(obj);
                // d_i = [d_(i-1), d_(i-1)]: 2^64 paths, 65 distinct vectors.
                let shared = (0..64).fold(base, |v, _| s.new_vector(&[v, v]).unwrap());
                (s.persist(good), s.persist(bad), s.persist(shared))
            });
            assert_eq!(rt.publish(&good), Ok(()));
            assert_eq!(rt.publish(&shared), Ok(()));
            let (path, found, _) = not_shareable(rt.publish(&bad).unwrap_err());
            assert_eq!(found, "object");
            assert_eq!(path.len(), DEPTH);
            assert!(path.iter().all(|step| *step == PathStep::Index(0)));
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn primitives_publish_and_foreign_handles_are_rejected() {
    let rt = Runtime::new();
    assert_eq!(rt.publish(&rt.root(num(1.0))), Ok(()));
    assert_eq!(rt.publish(&rt.root(Primitive::Null)), Ok(()));
    let other = Runtime::new();
    let foreign = other.scope(|s| {
        let v = s.new_vector(&[]).unwrap();
        s.persist(v)
    });
    assert_eq!(
        rt.publish(&foreign),
        Err(PublishError::Handle(HandleError::ForeignRuntime))
    );
}

/// clj-quickjs-65: strings are immutable primitives, so a string a task
/// computed (alone or inside a persistent collection) publishes without a
/// walk and another task reads and compares it by content. The programs are
/// JavaScript source (synthetic fixtures).
#[test]
fn strings_publish_as_persistent_values() {
    let rt = Runtime::new();
    let run_script = |name: &str, src: &str| {
        let exe = compile_script(name, src).unwrap();
        complete(rt.spawn(&exe, &[]).unwrap().run().unwrap())
    };
    let bare = run_script("bare", r#"let s = "pub"; s + "lished""#);
    assert_eq!(bare.view(), ValueView::String);
    assert_eq!(rt.publish(&bare), Ok(()));
    let map = run_script("map", r#"let s = "pub"; __rt.map("key", s + "lished")"#);
    assert_eq!(rt.publish(&map), Ok(()));

    // A consumer on another thread reads both, compares them with its own
    // literal and returns what it read.
    let consumer = compile_function(
        r#"function consume(m, s) {
             let got = __rt.get(m, "key");
             return __rt.vector(got, got === "published", s === got, s == "published");
           }"#,
    )
    .unwrap();
    let task = rt.spawn(&consumer, &[&map, &bare]).unwrap();
    let result = thread::scope(|t| t.spawn(|| complete(task.run().unwrap())).join().unwrap());
    let received = rt.scope(|s| {
        let items = s.items(s.local(&result).unwrap()).unwrap();
        for flag in &items[1..] {
            assert_eq!(s.view(*flag), ValueView::Bool(true));
        }
        s.persist(items[0])
    });
    assert_eq!(received.string().unwrap(), "published");
    assert_eq!(received.strict_equals(&bare), Ok(true), "equal by content");
    assert_eq!(rt.publish(&received), Ok(()));
}

/// Ordinary objects (clj-quickjs-114) are task-owned mutation: a value that
/// reaches one is rejected, and the path continues into the object's
/// property values to the innermost object along the first such chain
/// (`.property(name)` steps; `crate::value::share`, "The check"). A
/// closure that captured a `const` bound to an object is rejected at that
/// capture. JavaScript source is a synthetic fixture, not light-cljs output.
#[test]
fn publishing_an_object_literal_is_rejected_with_property_path() {
    let rt = Runtime::new();
    let run = |src: &str| {
        let task = rt
            .spawn(&compile_script("objects", src).unwrap(), &[])
            .unwrap();
        (complete(task.run().unwrap()), task.id())
    };
    let (holder, producer) = run("__rt.vector(1, {inner: {}})");
    let (path, found, owner) = not_shareable(rt.publish(&holder).unwrap_err());
    assert_eq!(
        path,
        vec![PathStep::Index(1), PathStep::Property("inner".to_string())]
    );
    assert_eq!((found, owner), ("object", Some(producer)));
    assert_eq!(
        rt.publish(&holder).unwrap_err().to_string(),
        format!(
            "cannot publish: value[1].property(inner) is a task-owned object \
             (owned by {producer}), which is not shareable"
        )
    );

    // The chain follows the first object-valued property in creation order
    // and stops at an object with none (or at a cycle).
    let (deep, _) = run("let o = {n: 1, a: {s: 'x'}, b: {c: {}}}; o.a.back = o; \
         __rt.map('k', o)");
    let (path, ..) = not_shareable(rt.publish(&deep).unwrap_err());
    assert_eq!(
        path,
        vec![
            PathStep::MapValue(KeyName::Primitive(Primitive::String("k".into()))),
            PathStep::Property("a".to_string()),
        ]
    );
    // A numeric key is the ToString of its value, the same property as the
    // equal string key (so a later one redefines it in place).
    for (src, name) in [
        ("__rt.vector({1.50: {}})", "1.5"),
        ("__rt.vector({0x10: {}})", "16"),
        ("__rt.vector({1e21: {}})", "1e+21"),
        ("__rt.vector({.5: 1, x: 2, '0.5': {}})", "0.5"),
        ("__rt.vector({'16': 1, x: 2, 0x10: {}})", "16"),
    ] {
        let (value, _) = run(src);
        let (path, ..) = not_shareable(rt.publish(&value).unwrap_err());
        assert_eq!(
            path,
            vec![PathStep::Index(0), PathStep::Property(name.to_string())],
            "{src}"
        );
    }
    // ... so a later string key replaces the value of an equal numeric key.
    let (value, _) = run("__rt.vector({0x10: {}, 1.50: {}, '16': 1, '1.5': 2})");
    let (path, ..) = not_shareable(rt.publish(&value).unwrap_err());
    assert_eq!(path, vec![PathStep::Index(0)]);
    // A property holding a builder ends the chain at the builder.
    let (built, _) = run("__rt.vector({b: __rt.vectorBuilder()})");
    let (path, found, _) = not_shareable(rt.publish(&built).unwrap_err());
    assert_eq!(
        (path, found),
        (
            vec![PathStep::Index(0), PathStep::Property("b".to_string())],
            "vector builder"
        )
    );

    // A closure over a `const` holding an object captures the object by
    // value: rejected at the capture.
    let (closure, producer) =
        run("function make() { const o = {}; return function () { return o; }; } make()");
    assert_eq!(closure.view(), ValueView::Object);
    assert_eq!(
        not_shareable(rt.publish(&closure).unwrap_err()),
        (vec![PathStep::Capture(0)], "object", Some(producer))
    );
    // An object alone is rejected at the value itself.
    let (alone, producer) = run("({x: 1})");
    assert_eq!(
        not_shareable(rt.publish(&alone).unwrap_err()),
        (vec![], "object", Some(producer))
    );
}

/// An ordinary object is task-owned (clj-quickjs-114): handed to another
/// task anyway (`spawn` does not check its arguments), it can be neither
/// read nor written there, and the failure is not a catchable JavaScript
/// error. An object the embedder created has no owner task: any task reads
/// it, none writes it.
#[test]
fn objects_are_read_and_written_only_by_their_owner() {
    let rt = Runtime::new();
    let producer = rt
        .spawn(&compile_script("make", "({x: 1})").unwrap(), &[])
        .unwrap();
    let object = complete(producer.run().unwrap());
    let read = compile_function("function f(o) { return o.x; }").unwrap();
    let write = compile_function(
        "function f(o) { try { o.x = 2; } catch (e) { return 'caught'; } return 0; }",
    )
    .unwrap();
    let outcome = |exe: &Executable, arg: &Rooted| {
        let task = rt.spawn(exe, &[arg]).unwrap();
        (task.run().unwrap(), task.id())
    };
    for exe in [&read, &write] {
        match outcome(exe, &object) {
            (RunOutcome::Failed(e), caller) => assert_eq!(
                e.kind(),
                VmErrorKind::ForeignObject {
                    owner: Some(producer.id()),
                    caller
                },
                "{e}"
            ),
            other => panic!("expected ForeignObject, got {other:?}"),
        }
    }

    let host = rt.scope(|s| {
        let o = s.new_object();
        s.persist(o)
    });
    match outcome(&read, &host).0 {
        RunOutcome::Complete(v) => assert_eq!(v.view(), ValueView::Undefined),
        other => panic!("expected Complete, got {other:?}"),
    }
    match outcome(&write, &host) {
        (RunOutcome::Failed(e), caller) => assert_eq!(
            e.kind(),
            VmErrorKind::ForeignObject {
                owner: None,
                caller
            }
        ),
        other => panic!("expected ForeignObject, got {other:?}"),
    }
}

/// Built-in globals and the realm intrinsics (clj-quickjs-117): a closure
/// that reads `Object` and calls `Object.is` captures nothing (the name is
/// resolved through the runtime's realm when it runs), publishes, and runs
/// in another task on another thread. The intrinsics are shared persistent
/// values, so a closure that captured one by value publishes too, and so
/// do the intrinsics themselves.
#[test]
fn closures_using_builtin_globals_publish() {
    let rt = Runtime::new();
    let produce_value = |src: &str| {
        let task = rt
            .spawn(&compile_script("make", src).unwrap(), &[])
            .unwrap();
        (complete(task.run().unwrap()), task.id())
    };
    let call = compile_function("function call(g) { return g(NaN); }").unwrap();
    for src in [
        // Captures nothing.
        "function make() { return function (x) { \
           return Object.is(x, x) && typeof Object === 'function' ? ({}).toString() : 'no'; }; } \
         make()",
        // Captures the intrinsics by value.
        "function make() { const O = Object, p = O.prototype; \
           return function (x) { return O.is(x, x) && p === Object.prototype \
             ? O().toString() : 'no'; }; } \
         make()",
    ] {
        let (closure, _) = produce_value(src);
        assert_eq!(closure.view(), ValueView::Object);
        rt.publish(&closure)
            .unwrap_or_else(|e| panic!("{src}: {e}"));
        let task = rt.spawn(&call, &[&closure]).unwrap();
        let result = thread::scope(|t| t.spawn(|| complete(task.run().unwrap())).join().unwrap());
        assert_eq!(
            result.string().map(|s| s.to_string()).as_deref(),
            Some("[object Object]"),
            "{src}"
        );
    }
    // The intrinsics themselves are shareable; an object that inherits from
    // one is still task-owned.
    for src in [
        "Object",
        "Object.prototype",
        "Object.prototype.toString",
        "Object.is",
    ] {
        let (value, _) = produce_value(src);
        rt.publish(&value).unwrap_or_else(|e| panic!("{src}: {e}"));
    }
    for src in ["({})", "Object()"] {
        let (object, producer) = produce_value(src);
        assert_eq!(
            not_shareable(rt.publish(&object).unwrap_err()),
            (vec![], "object", Some(producer)),
            "{src}"
        );
    }
}
