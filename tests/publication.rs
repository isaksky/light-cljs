//! Publication of values from one task to another through the embedding
//! API (pellet clj-quickjs-18): shareable graphs pass by identity, graphs
//! reaching task-owned mutation are rejected with the offending path.
//!
//! Form-like values are synthetic fixtures, not light-cljs output; numbers
//! stand in for symbols. Programs are hand-assembled bytecode: early VM
//! tests, not JavaScript source support.

use std::thread;

use clj_quickjs::{
    Assembler, Executable, HandleError, KeyName, Local, Opcode, PathStep, Primitive, Provenance,
    PublishError, Rooted, RunOutcome, Runtime, Scope, TaskId,
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
