//! Collection intrinsics and task-owned transient builders through the
//! embedding API (pellet clj-quickjs-19).
//!
//! Early VM tests: every program is hand-assembled bytecode, not JavaScript
//! source support. Collections built by the embedder are synthetic fixtures,
//! not light-cljs output.

use std::thread;

use big_boy_pants_js::{
    Assembler, Budget, BuilderError, CollectionError, Executable, Opcode, PersistentKind,
    Primitive, Rooted, RunError, RunOutcome, Runtime, Task, TaskStatus, ValueView, VmError,
    VmErrorKind,
};

fn num(n: f64) -> Primitive {
    Primitive::Number(n)
}

fn complete(outcome: RunOutcome) -> Rooted {
    match outcome {
        RunOutcome::Complete(v) => v,
        other => panic!("expected Complete, got {other:?}"),
    }
}

fn failed(outcome: RunOutcome) -> VmError {
    match outcome {
        RunOutcome::Failed(e) => e,
        other => panic!("expected Failed, got {other:?}"),
    }
}

/// Assemble a program with `params` parameters and 4 locals.
fn program(params: u16, build: impl FnOnce(&mut Assembler)) -> Executable {
    let mut asm = Assembler::new();
    build(&mut asm);
    asm.finish("intrinsics", params, 4).unwrap()
}

/// Run a parameterless program to its end.
fn eval(rt: &Runtime, build: impl FnOnce(&mut Assembler)) -> RunOutcome {
    rt.spawn(&program(0, build), &[]).unwrap().run().unwrap()
}

/// Push `[10, 20, 30]`.
fn vec3(asm: &mut Assembler) {
    asm.constant(num(10.0))
        .constant(num(20.0))
        .constant(num(30.0))
        .new_vector(3);
}

/// Push `{1: 10, 2: 20}`.
fn map2(asm: &mut Assembler) {
    asm.constant(num(1.0))
        .constant(num(10.0))
        .constant(num(2.0))
        .constant(num(20.0))
        .new_map(2);
}

fn numbers(rt: &Runtime, v: &Rooted) -> Vec<f64> {
    rt.scope(|s| {
        let local = s.local(v).unwrap();
        s.items(local)
            .unwrap()
            .into_iter()
            .map(|item| match s.view(item) {
                ValueView::Number(n) => n,
                other => panic!("not a number: {other:?}"),
            })
            .collect()
    })
}

fn map_get(rt: &Runtime, m: &Rooted, key: f64) -> Option<ValueView> {
    rt.scope(|s| {
        let (m, k) = (s.local(m).unwrap(), s.primitive(num(key)));
        s.get(m, k).unwrap().map(|v| s.view(v))
    })
}

fn count(rt: &Runtime, c: &Rooted) -> usize {
    rt.scope(|s| s.count(s.local(c).unwrap()).unwrap())
}

#[test]
fn constructors_build_vectors_and_maps_from_operands() {
    let rt = Runtime::new();
    let v = complete(eval(&rt, |a| {
        vec3(a);
        a.op(Opcode::Return);
    }));
    assert_eq!(numbers(&rt, &v), [10.0, 20.0, 30.0]);
    let empty = complete(eval(&rt, |a| {
        a.new_vector(0).op(Opcode::Return);
    }));
    assert_eq!(numbers(&rt, &empty), [] as [f64; 0]);
    // A later duplicate key wins.
    let m = complete(eval(&rt, |a| {
        a.constant(num(1.0)).constant(num(10.0));
        a.constant(num(1.0)).constant(num(30.0));
        a.new_map(2).op(Opcode::Return);
    }));
    let kind = rt.scope(|s| s.persistent_kind(s.local(&m).unwrap()));
    assert_eq!(kind, Some(PersistentKind::Map));
    assert_eq!(count(&rt, &m), 1);
    assert_eq!(map_get(&rt, &m, 1.0), Some(ValueView::Number(30.0)));
}

#[test]
fn lookups_count_nth_get_and_get_or() {
    let rt = Runtime::new();
    let value = |build: fn(&mut Assembler)| complete(eval(&rt, build)).view();
    let n = ValueView::Number;
    assert_eq!(
        value(|a| {
            vec3(a);
            a.op(Opcode::Count).op(Opcode::Return);
        }),
        n(3.0)
    );
    assert_eq!(
        value(|a| {
            map2(a);
            a.op(Opcode::Count).op(Opcode::Return);
        }),
        n(2.0)
    );
    assert_eq!(
        value(|a| {
            vec3(a);
            a.constant(num(1.0)).op(Opcode::Nth).op(Opcode::Return);
        }),
        n(20.0)
    );
    // -0 is index 0.
    assert_eq!(
        value(|a| {
            vec3(a);
            a.constant(num(-0.0)).op(Opcode::Get).op(Opcode::Return);
        }),
        n(10.0)
    );
    assert_eq!(
        value(|a| {
            map2(a);
            a.constant(num(2.0)).op(Opcode::Get).op(Opcode::Return);
        }),
        n(20.0)
    );
    assert_eq!(
        value(|a| {
            map2(a);
            a.constant(num(9.0))
                .constant(Primitive::Null)
                .op(Opcode::GetOr)
                .op(Opcode::Return);
        }),
        ValueView::Null
    );
    assert_eq!(
        value(|a| {
            vec3(a);
            a.constant(num(7.0))
                .constant(num(-1.0))
                .op(Opcode::GetOr)
                .op(Opcode::Return);
        }),
        n(-1.0)
    );
    assert_eq!(
        value(|a| {
            map2(a);
            a.constant(num(1.0))
                .constant(num(-1.0))
                .op(Opcode::GetOr)
                .op(Opcode::Return);
        }),
        n(10.0)
    );
}

#[test]
fn persistent_updates_leave_inputs_unchanged() {
    let rt = Runtime::new();
    // `function (v) { return [assoc(v, 0, 1), assoc(v, 3, 40), conj(v, 50), pop_last(v), v] }`
    let exe = program(1, |a| {
        a.get_local(0)
            .constant(num(0.0))
            .constant(num(1.0))
            .op(Opcode::Assoc);
        a.get_local(0)
            .constant(num(3.0))
            .constant(num(40.0))
            .op(Opcode::Assoc);
        a.get_local(0).constant(num(50.0)).op(Opcode::Conj);
        a.get_local(0).op(Opcode::PopLast);
        a.get_local(0).new_vector(5).op(Opcode::Return);
    });
    let input = rt.scope(|s| {
        let items: Vec<_> = [10.0, 20.0, 30.0]
            .into_iter()
            .map(|x| s.primitive(num(x)))
            .collect();
        let v = s.new_vector(&items).unwrap();
        s.persist(v)
    });
    let out = complete(rt.spawn(&exe, &[&input]).unwrap().run().unwrap());
    let versions: Vec<Vec<f64>> = rt.scope(|s| {
        let parts = s.items(s.local(&out).unwrap()).unwrap();
        assert!(s.same_object(parts[4], s.local(&input).unwrap()));
        parts.iter().map(|&p| numbers(&rt, &s.persist(p))).collect()
    });
    assert_eq!(
        versions,
        [
            vec![1.0, 20.0, 30.0],
            vec![10.0, 20.0, 30.0, 40.0],
            vec![10.0, 20.0, 30.0, 50.0],
            vec![10.0, 20.0],
            vec![10.0, 20.0, 30.0],
        ]
    );

    // Map assoc/dissoc, and no-op updates return the same object (`===`).
    let exe = program(0, |a| {
        map2(a);
        a.set_local(0);
        // [dissoc(m, 1), assoc(m, 3, 30), assoc(m, 1, 10) === m, dissoc(m, 9) === m]
        a.get_local(0).constant(num(1.0)).op(Opcode::Dissoc);
        a.get_local(0)
            .constant(num(3.0))
            .constant(num(30.0))
            .op(Opcode::Assoc);
        a.get_local(0)
            .constant(num(1.0))
            .constant(num(10.0))
            .op(Opcode::Assoc)
            .get_local(0)
            .op(Opcode::StrictEq);
        a.get_local(0)
            .constant(num(9.0))
            .op(Opcode::Dissoc)
            .get_local(0)
            .op(Opcode::StrictEq);
        a.new_vector(4).op(Opcode::Return);
    });
    let out = complete(rt.spawn(&exe, &[]).unwrap().run().unwrap());
    rt.scope(|s| {
        let parts = s.items(s.local(&out).unwrap()).unwrap();
        let (removed, added) = (s.persist(parts[0]), s.persist(parts[1]));
        assert_eq!(count(&rt, &removed), 1);
        assert_eq!(map_get(&rt, &removed, 1.0), None);
        assert_eq!(count(&rt, &added), 3);
        assert_eq!(map_get(&rt, &added, 3.0), Some(ValueView::Number(30.0)));
        assert_eq!(s.view(parts[2]), ValueView::Bool(true));
        assert_eq!(s.view(parts[3]), ValueView::Bool(true));
    });
}

/// An error case: program prefix, expected error, failing opcode.
type Case = (fn(&mut Assembler), CollectionError, Opcode);

#[test]
fn intrinsic_errors_fail_the_task_without_panicking() {
    use CollectionError::*;
    let wrong = |expected, found| WrongType { expected, found };
    let cases: Vec<Case> = vec![
        // bad index
        (
            |a| {
                vec3(a);
                a.constant(num(1.5)).op(Opcode::Nth);
            },
            InvalidIndex,
            Opcode::Nth,
        ),
        (
            |a| {
                vec3(a);
                a.constant(num(-1.0)).op(Opcode::Get);
            },
            InvalidIndex,
            Opcode::Get,
        ),
        (
            |a| {
                vec3(a);
                a.constant(Primitive::Null)
                    .constant(num(0.0))
                    .op(Opcode::GetOr);
            },
            InvalidIndex,
            Opcode::GetOr,
        ),
        (
            |a| {
                vec3(a);
                a.constant(num(f64::NAN))
                    .constant(num(0.0))
                    .op(Opcode::Assoc);
            },
            InvalidIndex,
            Opcode::Assoc,
        ),
        (
            |a| {
                vec3(a);
                a.constant(num(3.0)).op(Opcode::Nth);
            },
            IndexOutOfBounds { index: 3, count: 3 },
            Opcode::Nth,
        ),
        (
            |a| {
                vec3(a);
                a.constant(num(5.0)).op(Opcode::Get);
            },
            IndexOutOfBounds { index: 5, count: 3 },
            Opcode::Get,
        ),
        (
            |a| {
                vec3(a);
                a.constant(num(4.0)).constant(num(0.0)).op(Opcode::Assoc);
            },
            IndexOutOfBounds { index: 4, count: 3 },
            Opcode::Assoc,
        ),
        (
            |a| {
                a.new_vector(0).op(Opcode::PopLast);
            },
            IndexOutOfBounds { index: 0, count: 0 },
            Opcode::PopLast,
        ),
        // missing key
        (
            |a| {
                map2(a);
                a.constant(num(3.0)).op(Opcode::Get);
            },
            MissingKey,
            Opcode::Get,
        ),
        // wrong type
        (
            |a| {
                a.op(Opcode::NewObject).op(Opcode::Count);
            },
            wrong("vector or map", "object"),
            Opcode::Count,
        ),
        (
            |a| {
                a.constant(num(1.0)).constant(num(0.0)).op(Opcode::Get);
            },
            wrong("vector or map", "number"),
            Opcode::Get,
        ),
        (
            |a| {
                map2(a);
                // The operand kind is checked before the index.
                a.constant(num(1.5)).op(Opcode::Nth);
            },
            wrong("vector", "map"),
            Opcode::Nth,
        ),
        (
            |a| {
                vec3(a);
                a.constant(num(0.0)).op(Opcode::Dissoc);
            },
            wrong("map", "vector"),
            Opcode::Dissoc,
        ),
        (
            |a| {
                map2(a);
                a.constant(num(0.0)).op(Opcode::Conj);
            },
            wrong("vector", "map"),
            Opcode::Conj,
        ),
        (
            |a| {
                a.op(Opcode::Undefined).op(Opcode::PopLast);
            },
            wrong("vector", "undefined"),
            Opcode::PopLast,
        ),
        (
            |a| {
                a.constant(Primitive::Bool(true))
                    .constant(num(0.0))
                    .constant(num(0.0))
                    .op(Opcode::Assoc);
            },
            wrong("vector or map", "boolean"),
            Opcode::Assoc,
        ),
        (
            |a| {
                a.op(Opcode::NewVectorBuilder).op(Opcode::Count);
            },
            wrong("vector or map", "vector builder"),
            Opcode::Count,
        ),
    ];
    let rt = Runtime::new();
    for (i, (build, expected, opcode)) in cases.into_iter().enumerate() {
        let task = rt
            .spawn(
                &program(0, |a| {
                    build(a);
                    a.op(Opcode::Return);
                }),
                &[],
            )
            .unwrap();
        let err = failed(task.run().unwrap());
        assert_eq!(
            err.kind(),
            VmErrorKind::Collection(expected.clone()),
            "case {i}"
        );
        assert_eq!(err.message(), expected.to_string(), "case {i}");
        assert_eq!(err.diagnostic().opcode, Some(opcode), "case {i}");
        assert_eq!(err.diagnostic().task, task.id());
        assert_eq!(task.status(), TaskStatus::Finished);
        assert_eq!(task.run().unwrap_err(), RunError::Finished);
    }
}

#[test]
fn constructor_operand_counts_are_validated_at_load() {
    let mut asm = Assembler::new();
    asm.constant(num(1.0)).new_vector(2).op(Opcode::Return);
    let err = asm.finish("short", 0, 0).unwrap_err();
    assert!(
        matches!(
            err.kind,
            big_boy_pants_js::ValidationErrorKind::StackUnderflow {
                opcode: Opcode::NewVector,
                depth: 1
            }
        ),
        "{err:?}"
    );
    let mut asm = Assembler::new();
    asm.constant(num(1.0))
        .constant(num(2.0))
        .constant(num(3.0))
        .new_map(2)
        .op(Opcode::Return);
    let err = asm.finish("odd", 0, 0).unwrap_err();
    assert!(
        matches!(
            err.kind,
            big_boy_pants_js::ValidationErrorKind::StackUnderflow {
                opcode: Opcode::NewMap,
                depth: 3
            }
        ),
        "{err:?}"
    );
}

/// `function (n) { let b = vector_builder(); for (let i = 0; i < n; i++) conj!(b, i * i);
///   assoc!(b, 0, -1); return persistent!(b); }`, plus `freeze` again when
/// `refreeze` is set.
fn squares(refreeze: bool) -> Executable {
    let mut asm = Assembler::new();
    let (top, done) = (asm.label(), asm.label());
    asm.op(Opcode::NewVectorBuilder).set_local(1);
    asm.constant(num(0.0)).set_local(2);
    asm.bind(top);
    asm.get_local(2)
        .get_local(0)
        .op(Opcode::Lt)
        .jump_if_false(done);
    asm.get_local(1)
        .get_local(2)
        .get_local(2)
        .op(Opcode::Mul)
        .op(Opcode::BuilderConj)
        .op(Opcode::Pop);
    asm.get_local(2)
        .constant(num(1.0))
        .op(Opcode::Add)
        .set_local(2);
    asm.jump(top);
    asm.bind(done);
    asm.get_local(1)
        .constant(num(0.0))
        .constant(num(-1.0))
        .op(Opcode::BuilderAssoc);
    asm.op(Opcode::Freeze);
    if refreeze {
        asm.op(Opcode::Pop).get_local(1).op(Opcode::Freeze);
    }
    asm.op(Opcode::Return);
    asm.finish("squares", 1, 3).unwrap()
}

#[test]
fn vector_builder_mutates_in_place_and_freezes_to_a_persistent_vector() {
    let rt = Runtime::new();
    let n = rt.root(num(40.0));
    let v = complete(rt.spawn(&squares(false), &[&n]).unwrap().run().unwrap());
    let kind = rt.scope(|s| s.persistent_kind(s.local(&v).unwrap()));
    assert_eq!(kind, Some(PersistentKind::Vector));
    let mut expected: Vec<f64> = (0..40).map(|i| f64::from(i * i)).collect();
    expected[0] = -1.0;
    assert_eq!(numbers(&rt, &v), expected);
}

#[test]
fn map_builder_assoc_overwrites_and_freezes_to_a_persistent_map() {
    let rt = Runtime::new();
    let m = complete(eval(&rt, |a| {
        a.op(Opcode::NewMapBuilder);
        for (k, v) in [(1.0, 10.0), (2.0, 20.0), (1.0, 11.0)] {
            a.constant(num(k)).constant(num(v)).op(Opcode::BuilderAssoc);
        }
        a.op(Opcode::Freeze).op(Opcode::Return);
    }));
    assert_eq!(count(&rt, &m), 2);
    assert_eq!(map_get(&rt, &m, 1.0), Some(ValueView::Number(11.0)));
    assert_eq!(map_get(&rt, &m, 2.0), Some(ValueView::Number(20.0)));
}

#[test]
fn vector_builder_assoc_appends_at_count_and_replaces_below_it() {
    let rt = Runtime::new();
    let v = complete(eval(&rt, |a| {
        a.op(Opcode::NewVectorBuilder);
        for (i, x) in [(0.0, 1.0), (1.0, 2.0), (0.0, 3.0), (2.0, 4.0)] {
            a.constant(num(i)).constant(num(x)).op(Opcode::BuilderAssoc);
        }
        a.op(Opcode::Freeze).op(Opcode::Return);
    }));
    assert_eq!(numbers(&rt, &v), [3.0, 2.0, 4.0]);
}

/// `function () { let m = map_builder(); for (let i = 0; i < 10; i++) assoc!(m, {}, {});
///   return persistent!(m); }`
fn object_map() -> Executable {
    let mut asm = Assembler::new();
    let (top, done) = (asm.label(), asm.label());
    asm.op(Opcode::NewMapBuilder).set_local(0);
    asm.constant(num(0.0)).set_local(1);
    asm.bind(top);
    asm.get_local(1)
        .constant(num(10.0))
        .op(Opcode::Lt)
        .jump_if_false(done);
    asm.get_local(0)
        .op(Opcode::NewObject)
        .op(Opcode::NewObject)
        .op(Opcode::BuilderAssoc)
        .op(Opcode::Pop);
    asm.get_local(1)
        .constant(num(1.0))
        .op(Opcode::Add)
        .set_local(1);
    asm.jump(top);
    asm.bind(done);
    asm.get_local(0).op(Opcode::Freeze).op(Opcode::Return);
    asm.finish("object-map", 0, 2).unwrap()
}

#[test]
fn map_builder_keys_and_values_are_traced_across_yields() {
    let rt = Runtime::new();
    let task = rt.spawn(&object_map(), &[]).unwrap();
    // Collects (reclaiming nothing) after every bounded run.
    let (m, runs) = drive_across_threads(&rt, &task, 4);
    assert!(runs > 10, "the build was split over many runs ({runs})");
    assert_eq!(count(&rt, &m), 10);
    let views = rt.scope(|s| {
        let entries = s.entries(s.local(&m).unwrap()).unwrap();
        entries
            .iter()
            .flat_map(|&(k, v)| [s.view(k), s.view(v)])
            .collect::<Vec<_>>()
    });
    assert_eq!(views, vec![ValueView::Object; 20]);
    // Map, 20 objects and the frozen builder; only the builder is garbage.
    drop(task);
    assert_eq!(rt.collect().unwrap().reclaimed, 1);
    assert_eq!(rt.heap_stats().objects, 21);
}

#[test]
fn builder_use_after_freeze_is_rejected() {
    let rt = Runtime::new();
    let n = rt.root(num(3.0));
    let err = failed(rt.spawn(&squares(true), &[&n]).unwrap().run().unwrap());
    assert_eq!(err.kind(), VmErrorKind::Builder(BuilderError::Frozen));
    assert_eq!(err.diagnostic().opcode, Some(Opcode::Freeze));

    let err = failed(eval(&rt, |a| {
        a.op(Opcode::NewMapBuilder)
            .op(Opcode::Dup)
            .op(Opcode::Freeze);
        a.op(Opcode::Pop)
            .constant(num(1.0))
            .constant(num(2.0))
            .op(Opcode::BuilderAssoc)
            .op(Opcode::Return);
    }));
    assert_eq!(err.kind(), VmErrorKind::Builder(BuilderError::Frozen));
    assert_eq!(err.diagnostic().opcode, Some(Opcode::BuilderAssoc));
    assert_eq!(err.message(), "builder has already been frozen");
}

#[test]
fn builder_operand_errors_follow_the_contract() {
    use CollectionError::*;
    let rt = Runtime::new();
    let cases: Vec<Case> = vec![
        (
            |a| {
                vec3(a);
                a.constant(num(1.0)).op(Opcode::BuilderConj);
            },
            WrongType {
                expected: "vector builder",
                found: "vector",
            },
            Opcode::BuilderConj,
        ),
        (
            |a| {
                a.op(Opcode::NewMapBuilder)
                    .constant(num(1.0))
                    .op(Opcode::BuilderConj);
            },
            WrongType {
                expected: "vector builder",
                found: "map builder",
            },
            Opcode::BuilderConj,
        ),
        (
            |a| {
                a.constant(num(1.0)).op(Opcode::Freeze);
            },
            WrongType {
                expected: "builder",
                found: "number",
            },
            Opcode::Freeze,
        ),
        (
            |a| {
                map2(a);
                a.constant(num(1.0))
                    .constant(num(1.0))
                    .op(Opcode::BuilderAssoc);
            },
            WrongType {
                expected: "builder",
                found: "map",
            },
            Opcode::BuilderAssoc,
        ),
        (
            |a| {
                a.op(Opcode::NewVectorBuilder)
                    .constant(num(1.0))
                    .constant(num(0.0))
                    .op(Opcode::BuilderAssoc);
            },
            IndexOutOfBounds { index: 1, count: 0 },
            Opcode::BuilderAssoc,
        ),
        (
            |a| {
                a.op(Opcode::NewVectorBuilder)
                    .constant(num(0.5))
                    .constant(num(0.0))
                    .op(Opcode::BuilderAssoc);
            },
            InvalidIndex,
            Opcode::BuilderAssoc,
        ),
    ];
    for (i, (build, expected, opcode)) in cases.into_iter().enumerate() {
        let err = failed(eval(&rt, |a| {
            build(a);
            a.op(Opcode::Return);
        }));
        assert_eq!(err.kind(), VmErrorKind::Collection(expected), "case {i}");
        assert_eq!(err.diagnostic().opcode, Some(opcode), "case {i}");
    }
}

/// `function (b) { return builder_conj(b, 1) }` and `function (b) { return freeze(b) }`
fn use_builder(op: Opcode) -> Executable {
    program(1, |a| {
        a.get_local(0);
        if op == Opcode::BuilderConj {
            a.constant(num(1.0));
        }
        a.op(op).op(Opcode::Return);
    })
}

#[test]
fn builders_of_a_finished_task_are_released() {
    let rt = Runtime::new();
    // The owner returns its open builder (holding one object) and finishes.
    let owner = rt
        .spawn(
            &program(0, |a| {
                a.op(Opcode::NewVectorBuilder)
                    .op(Opcode::NewObject)
                    .op(Opcode::BuilderConj)
                    .op(Opcode::Return);
            }),
            &[],
        )
        .unwrap();
    let builder = complete(owner.run().unwrap());
    assert_eq!(builder.view(), ValueView::Object);
    for op in [Opcode::BuilderConj, Opcode::Freeze] {
        let other = rt.spawn(&use_builder(op), &[&builder]).unwrap();
        let err = failed(other.run().unwrap());
        assert_eq!(
            err.kind(),
            VmErrorKind::Builder(BuilderError::OwnerFinished { owner: owner.id() })
        );
        assert_eq!(err.diagnostic().task, other.id());
    }
    // Release dropped the draft: the builder object itself stays alive
    // through the root, its former contents are reclaimed.
    drop(owner);
    assert_eq!(rt.heap_stats().objects, 2);
    let stats = rt.collect().unwrap();
    assert_eq!((stats.live, stats.reclaimed), (1, 1));
    assert!(builder.downgrade().is_live());
}

/// `function (n) { let b = vector_builder(); for (let i = 0; i < n; i++) conj!(b, new Object());
///   return persistent!(b); }`
fn objects(n: f64) -> Executable {
    let mut asm = Assembler::new();
    let (top, done) = (asm.label(), asm.label());
    asm.op(Opcode::NewVectorBuilder).set_local(0);
    asm.constant(num(0.0)).set_local(1);
    asm.bind(top);
    asm.get_local(1)
        .constant(num(n))
        .op(Opcode::Lt)
        .jump_if_false(done);
    asm.get_local(0)
        .op(Opcode::NewObject)
        .op(Opcode::BuilderConj)
        .op(Opcode::Pop);
    asm.get_local(1)
        .constant(num(1.0))
        .op(Opcode::Add)
        .set_local(1);
    asm.jump(top);
    asm.bind(done);
    asm.get_local(0).op(Opcode::Freeze).op(Opcode::Return);
    asm.finish("objects", 0, 2).unwrap()
}

/// Drive `task` one bounded run per freshly spawned OS thread, collecting
/// between runs, until it completes. Returns the result and the run count.
fn drive_across_threads(rt: &Runtime, task: &Task, budget: u64) -> (Rooted, usize) {
    let mut runs = 0;
    loop {
        runs += 1;
        assert!(runs < 500, "task did not finish");
        let (outcome, thread) = thread::scope(|s| {
            s.spawn(|| {
                let outcome = task.run_with(Budget::instructions(budget), &mut NoEffects);
                (outcome, thread::current().id())
            })
            .join()
            .unwrap()
        });
        assert_ne!(thread, thread::current().id());
        match outcome.unwrap() {
            RunOutcome::Yielded => {
                // Builder contents are traced through the task's frame and
                // the builder object while the task is suspended.
                assert_eq!(rt.collect().unwrap().reclaimed, 0);
            }
            RunOutcome::Complete(v) => return (v, runs),
            other => panic!("{other:?}"),
        }
    }
}

struct NoEffects;

impl big_boy_pants_js::Host for NoEffects {
    fn effect(&mut self, _: big_boy_pants_js::HostCall) -> Result<Primitive, String> {
        Err("no effects expected".into())
    }
}

#[test]
fn builder_stays_usable_by_its_owner_across_yields_and_threads() {
    let rt = Runtime::new();
    let task = rt.spawn(&objects(20.0), &[]).unwrap();
    let (v, runs) = drive_across_threads(&rt, &task, 3);
    assert!(runs > 20, "the build was split over many runs ({runs})");
    let items = rt.scope(|s| {
        let items = s.items(s.local(&v).unwrap()).unwrap();
        let distinct = items
            .iter()
            .enumerate()
            .all(|(i, &a)| items[..i].iter().all(|&b| !s.same_object(a, b)));
        assert!(distinct, "twenty distinct objects");
        items.iter().map(|&i| s.view(i)).collect::<Vec<_>>()
    });
    assert_eq!(items, vec![ValueView::Object; 20]);

    // Builder (frozen, unreachable), vector and its 20 objects: only the
    // builder is garbage once the task is gone.
    drop(task);
    assert_eq!(rt.collect().unwrap().reclaimed, 1);
    assert_eq!(rt.heap_stats().objects, 21);
    drop(v);
    assert_eq!(rt.collect().unwrap().reclaimed, 21);
}
