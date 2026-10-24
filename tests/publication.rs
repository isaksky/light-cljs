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

/// Arrays (clj-quickjs-118) are task-owned mutation like ordinary objects:
/// a value that reaches one is rejected at the array, and the path
/// continues into its elements by index (`[i]`, [`PathStep::Element`]),
/// before its named properties, to the innermost task-owned object along
/// the first such chain. Handed to another task anyway, an array can be
/// neither read nor written there. JavaScript source is a synthetic
/// fixture, not light-cljs output.
#[test]
fn publishing_an_array_is_rejected_with_index_path() {
    let rt = Runtime::new();
    let run = |src: &str| {
        let task = rt
            .spawn(&compile_script("arrays", src).unwrap(), &[])
            .unwrap();
        (complete(task.run().unwrap()), task.id())
    };
    let (holder, producer) = run("__rt.vector(1, [2, , 'x', {}])");
    let (path, found, owner) = not_shareable(rt.publish(&holder).unwrap_err());
    assert_eq!(path, vec![PathStep::Index(1), PathStep::Element(3)]);
    assert_eq!((found, owner), ("object", Some(producer)));
    assert_eq!(
        rt.publish(&holder).unwrap_err().to_string(),
        format!(
            "cannot publish: value[1][3] is a task-owned object \
             (owned by {producer}), which is not shareable"
        )
    );
    // Elements in index order (sparse ones too), then named properties;
    // through objects and arrays alike.
    for (src, steps, found) in [
        (
            "let a = [1, [], {}]; a[9000] = {}; a.p = {}; __rt.vector(a)",
            vec![PathStep::Index(0), PathStep::Element(1)],
            "array",
        ),
        (
            "let a = [1]; a[9000] = [0, {}]; a.p = {}; __rt.vector(a)",
            vec![
                PathStep::Index(0),
                PathStep::Element(9000),
                PathStep::Element(1),
            ],
            "object",
        ),
        (
            "let a = [1, 2]; a.p = [3]; __rt.vector(a)",
            vec![PathStep::Index(0), PathStep::Property("p".to_string())],
            "array",
        ),
        (
            "__rt.map('k', {o: [[{}]]})",
            vec![
                PathStep::MapValue(KeyName::Primitive(Primitive::String("k".into()))),
                PathStep::Property("o".to_string()),
                PathStep::Element(0),
                PathStep::Element(0),
            ],
            "object",
        ),
    ] {
        let (value, producer) = run(src);
        assert_eq!(
            not_shareable(rt.publish(&value).unwrap_err()),
            (steps, found, Some(producer)),
            "{src}"
        );
    }
    // A cycle ends the chain; an array of primitives is rejected at itself.
    let (cycle, _) = run("let a = [0]; a[1] = a; __rt.vector(a)");
    let (path, found, _) = not_shareable(rt.publish(&cycle).unwrap_err());
    assert_eq!((path, found), (vec![PathStep::Index(0)], "array"));
    let (alone, producer) = run("[1, 2, 3]");
    assert_eq!(
        not_shareable(rt.publish(&alone).unwrap_err()),
        (vec![], "array", Some(producer))
    );
    assert_eq!(alone.view(), ValueView::Object);

    // Another task can neither read nor write it.
    let read = compile_function("function f(a) { return a[0]; }").unwrap();
    let write = compile_function("function f(a) { a[0] = 2; return 0; }").unwrap();
    let length = compile_function("function f(a) { return a.length; }").unwrap();
    let delete = compile_function("function f(a) { return delete a[0]; }").unwrap();
    for exe in [&read, &write, &length, &delete] {
        let task = rt.spawn(exe, &[&alone]).unwrap();
        match task.run().unwrap() {
            RunOutcome::Failed(e) => assert_eq!(
                e.kind(),
                VmErrorKind::ForeignObject {
                    owner: Some(producer),
                    caller: task.id()
                },
                "{e}"
            ),
            other => panic!("expected ForeignObject, got {other:?}"),
        }
    }
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
    let delete = compile_function("function f(o) { return delete o.x; }").unwrap();
    let outcome = |exe: &Executable, arg: &Rooted| {
        let task = rt.spawn(exe, &[arg]).unwrap();
        (task.run().unwrap(), task.id())
    };
    for exe in [&read, &write, &delete] {
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
    for exe in [&write, &delete] {
        match outcome(exe, &host) {
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

/// Function own properties (clj-quickjs-125; `docs/architecture.md`,
/// "Function own properties"): an untouched function publishes; one whose
/// properties were written, or whose lazy `prototype` was read, is rejected
/// at its task-owned property record (`.properties`), and the path goes on
/// into the record's property values. A published function is shared and
/// immutable: no task can give it properties or a prototype any more.
/// JavaScript source is a synthetic fixture, not light-cljs output.
#[test]
fn function_own_properties_follow_publication_rule() {
    let rt = Runtime::new();
    let run = |src: &str| {
        let task = rt
            .spawn(&compile_script("functions", src).unwrap(), &[])
            .unwrap();
        (complete(task.run().unwrap()), task.id())
    };
    // Untouched: publishes, and runs in another task on another thread.
    let (h, _) = run("function h() { return 7; } h");
    rt.publish(&h).unwrap();
    let call = compile_function("function call(g) { return g(); }").unwrap();
    let task = rt.spawn(&call, &[&h]).unwrap();
    let result = thread::scope(|t| t.spawn(|| complete(task.run().unwrap())).join().unwrap());
    assert_eq!(result.view(), ValueView::Number(7.0));

    // A `__proto__` assignment the accessor ignores (clj-quickjs-121)
    // writes no property: still publishable.
    let (h, _) = run("function h() {} h.__proto__ = 1; h");
    rt.publish(&h).unwrap();

    // A written property: rejected at the record.
    let (h, producer) = run("function h() {} h.x = 1; h");
    let err = rt.publish(&h).unwrap_err();
    assert_eq!(
        not_shareable(err.clone()),
        (vec![PathStep::FunctionProperties], "object", Some(producer))
    );
    assert_eq!(
        err.to_string(),
        format!(
            "cannot publish: value.properties is a task-owned object (owned by {producer}), \
             which is not shareable"
        )
    );
    // A read `prototype`: rejected at the prototype object in the record.
    let (h, producer) = run("function h() {} h.prototype; __rt.vector(1, h)");
    assert_eq!(
        not_shareable(rt.publish(&h).unwrap_err()),
        (
            vec![
                PathStep::Index(1),
                PathStep::FunctionProperties,
                PathStep::Property("prototype".to_string()),
            ],
            "object",
            Some(producer)
        )
    );
    // A property holding a shareable value still makes the function
    // task-owned: the record itself is mutable.
    let (h, _) = run("const h = () => 1; h.n = __rt.vector(1); h");
    assert_eq!(
        not_shareable(rt.publish(&h).unwrap_err()).0,
        vec![PathStep::FunctionProperties]
    );
    // An arrow has no `prototype`, so reading it (or any absent property)
    // creates nothing.
    let (h, _) = run("const h = () => 1; h.prototype; h.y; h");
    rt.publish(&h).unwrap();

    // Once published, a function is immutable: a write is a catchable
    // TypeError, and creating its prototype object fails explicitly; reads
    // of absent properties still answer.
    let (h, _) = run("function h() {} h");
    rt.publish(&h).unwrap();
    let probe = compile_function(
        "function probe(h) { let r = h.y === undefined ? 1 : 0; \
         try { h.x = 1; } catch (e) { r = r + 10; } return r + (h.x === undefined ? 100 : 0); }",
    )
    .unwrap();
    let task = rt.spawn(&probe, &[&h]).unwrap();
    assert_eq!(
        complete(task.run().unwrap()).view(),
        ValueView::Number(111.0)
    );
    let prototype = compile_function("function p(h) { return h.prototype; }").unwrap();
    match rt.spawn(&prototype, &[&h]).unwrap().run().unwrap() {
        RunOutcome::Failed(e) => {
            assert_eq!(e.kind(), VmErrorKind::UnsupportedOperand, "{e}");
            assert!(e.message().contains("published function"), "{e}");
        }
        other => panic!("expected Failed, got {other:?}"),
    }
    // `delete` from another task creates nothing (clj-quickjs-124): an
    // absent property is true, the non-configurable `prototype` a
    // catchable TypeError (strict code), and the function still publishes.
    let delete = compile_function(
        "function d(h) { let r = delete h.x ? 1 : 0; \
         try { delete h.prototype; } catch (e) { r = r + 10; } return r; }",
    )
    .unwrap();
    let task = rt.spawn(&delete, &[&h]).unwrap();
    assert_eq!(
        complete(task.run().unwrap()).view(),
        ValueView::Number(11.0)
    );
    rt.publish(&h).unwrap();
    // Assigning `prototype` is a write like any other: a catchable
    // TypeError on a published function or arrow.
    let assign = compile_function(
        "function assign(h) { try { h.prototype = 3; } catch (e) { return 1; } return 0; }",
    )
    .unwrap();
    for src in ["function h() {} h", "const h = () => 1; h"] {
        let (h, _) = run(src);
        rt.publish(&h).unwrap();
        let task = rt.spawn(&assign, &[&h]).unwrap();
        assert_eq!(
            complete(task.run().unwrap()).view(),
            ValueView::Number(1.0),
            "{src}"
        );
    }
    // Installing a function in an atom publishes it too.
    let (h, _) = run("function h() {} h");
    let _atom = rt.new_atom(&h).unwrap();
    let task = rt.spawn(&probe, &[&h]).unwrap();
    assert_eq!(
        complete(task.run().unwrap()).view(),
        ValueView::Number(111.0)
    );

    // Not published but handed over unchecked (`spawn` does not check its
    // arguments): only the creating task may write it, delete from it or
    // create its
    // prototype.
    let (h, producer) = run("function h() {} h");
    for exe in [&probe, &prototype, &delete] {
        let task = rt.spawn(exe, &[&h]).unwrap();
        match task.run().unwrap() {
            RunOutcome::Failed(e) => assert_eq!(
                e.kind(),
                VmErrorKind::ForeignObject {
                    owner: Some(producer),
                    caller: task.id()
                },
                "{e}"
            ),
            other => panic!("expected ForeignObject, got {other:?}"),
        }
    }
    // Nothing was attached, so it still publishes.
    rt.publish(&h).unwrap();
}

/// Extensibility and freezing across tasks (clj-quickjs-122;
/// `src/vm/integrity.rs`): freezing does not change ownership, so a frozen
/// object stays task-owned and is rejected by publication, and freezing a
/// function gives it a task-owned property record, which makes it
/// unpublishable. A published function is shared and immutable, so it
/// reports itself not extensible and frozen, and freezing it again
/// succeeds. Another task's object or recorded function is
/// `ForeignObject`. JavaScript source is a synthetic fixture, not
/// light-cljs output.
#[test]
fn freezing_keeps_ownership_and_published_functions_are_frozen() {
    let rt = Runtime::new();
    let run = |src: &str| {
        let task = rt
            .spawn(&compile_script("integrity", src).unwrap(), &[])
            .unwrap();
        (complete(task.run().unwrap()), task.id())
    };
    // A frozen object, array or function is still task-owned.
    let (o, producer) = run("Object.freeze({x: 1})");
    assert_eq!(
        not_shareable(rt.publish(&o).unwrap_err()),
        (vec![], "object", Some(producer))
    );
    let (a, producer) = run("Object.freeze([1])");
    assert_eq!(
        not_shareable(rt.publish(&a).unwrap_err()),
        (vec![], "array", Some(producer))
    );
    for src in [
        "const h = () => 1; Object.freeze(h)",
        "function h() {} Object.preventExtensions(h)",
    ] {
        let (h, producer) = run(src);
        let (path, found, owner) = not_shareable(rt.publish(&h).unwrap_err());
        assert_eq!(path[0], PathStep::FunctionProperties, "{src}");
        assert_eq!((found, owner), ("object", Some(producer)), "{src}");
    }

    // A published function reports frozen; freezing it changes nothing.
    let probe = compile_function(
        "function probe(h) { return !Object.isExtensible(h) && Object.isFrozen(h) \
         && Object.freeze(h) === h && Object.preventExtensions(h) === h \
         && Object.isFrozen(h) && h() === 7; }",
    )
    .unwrap();
    for src in ["function h() { return 7; } h", "const h = () => 7; h"] {
        let (h, _) = run(src);
        rt.publish(&h).unwrap();
        let task = rt.spawn(&probe, &[&h]).unwrap();
        assert_eq!(
            complete(task.run().unwrap()).view(),
            ValueView::Bool(true),
            "{src}"
        );
        // Still publishable: nothing was attached.
        rt.publish(&h).unwrap();
    }

    // Handed over unchecked (`spawn` does not check its arguments): only
    // the owner changes or reads the integrity of its objects.
    let (o, producer) = run("({x: 1})");
    let (h, creator) = run("function h() {} h");
    let (recorded, recorder) = run("const h = () => 1; h.x = 1; h");
    for (src, arg, owner) in [
        (
            "function f(o) { return Object.isExtensible(o); }",
            &o,
            producer,
        ),
        ("function f(o) { return Object.isFrozen(o); }", &o, producer),
        ("function f(o) { return Object.freeze(o); }", &o, producer),
        (
            "function f(o) { return Object.preventExtensions(o); }",
            &h,
            creator,
        ),
        (
            "function f(o) { return Object.isFrozen(o); }",
            &recorded,
            recorder,
        ),
    ] {
        let task = rt.spawn(&compile_function(src).unwrap(), &[arg]).unwrap();
        match task.run().unwrap() {
            RunOutcome::Failed(e) => assert_eq!(
                e.kind(),
                VmErrorKind::ForeignObject {
                    owner: Some(owner),
                    caller: task.id()
                },
                "{src}"
            ),
            other => panic!("{src}: expected ForeignObject, got {other:?}"),
        }
    }
    // Another task's function without a record reads as extensible, and
    // the failed change attached nothing: it still publishes.
    let read =
        compile_function("function f(h) { return Object.isExtensible(h) && !Object.isFrozen(h); }")
            .unwrap();
    let task = rt.spawn(&read, &[&h]).unwrap();
    assert_eq!(complete(task.run().unwrap()).view(), ValueView::Bool(true));
    rt.publish(&h).unwrap();
}
