//! Persistent vectors, maps and syntax occurrences through the embedding
//! API: identity sharing across tasks, persistent updates, collection, and
//! provenance separate from value equality.
//!
//! The form-like values are synthetic fixtures, not light-cljs output. No
//! strings exist yet, so numbers stand in for symbol names. Programs are
//! hand-assembled bytecode: early VM tests, not JavaScript source support.

use std::thread;

use clj_quickjs::{
    Assembler, CollectionError, Executable, Local, Opcode, PersistentKind, Primitive, Provenance,
    Rooted, RunOutcome, Runtime, Scope, ValueView,
};

fn complete(outcome: RunOutcome) -> Rooted {
    match outcome {
        RunOutcome::Complete(v) => v,
        other => panic!("expected Complete, got {other:?}"),
    }
}

fn at(line: u32, column: u32) -> Provenance {
    Provenance {
        source: 1,
        line,
        column,
    }
}

fn num<'s>(s: &Scope<'s>, n: f64) -> Local<'s> {
    s.primitive(Primitive::Number(n))
}

fn occ<'s>(s: &Scope<'s>, n: f64, p: Provenance) -> Local<'s> {
    let v = num(s, n);
    s.new_occurrence(v, p).unwrap()
}

// Stand-in symbol ids.
const LET: f64 = 100.0;
const X: f64 = 200.0;
const Y: f64 = 201.0;
const PLUS: f64 = 300.0;
// Field ids of the body record.
const OP: f64 = 0.0;
const ARGS: f64 = 1.0;

/// Build the synthetic form `(let [x 1 y 1] (+ x y))` on line `line`:
/// `[let-occ, [x 1 y 1], {OP: +, ARGS: [x y]}]`, every leaf an occurrence
/// with its own column. Returns (form, bindings vector, body record).
fn build_form<'s>(s: &Scope<'s>, line: u32) -> (Local<'s>, Local<'s>, Local<'s>) {
    let bindings = s
        .new_vector(&[
            occ(s, X, at(line, 7)),
            occ(s, 1.0, at(line, 9)),
            occ(s, Y, at(line, 11)),
            occ(s, 1.0, at(line, 13)),
        ])
        .unwrap();
    let args = s
        .new_vector(&[occ(s, X, at(line, 19)), occ(s, Y, at(line, 21))])
        .unwrap();
    let body = s
        .new_map(&[
            (num(s, OP), occ(s, PLUS, at(line, 17))),
            (num(s, ARGS), args),
        ])
        .unwrap();
    let form = s
        .new_vector(&[occ(s, LET, at(line, 2)), bindings, body])
        .unwrap();
    (form, bindings, body)
}

/// `function pick(form, sub, which) { if (which) return sub; return form; }`
fn pick() -> Executable {
    let mut asm = Assembler::new();
    let return_form = asm.label();
    asm.get_local(2).jump_if_false(return_form);
    asm.get_local(1).op(Opcode::Return);
    asm.bind(return_form);
    asm.get_local(0).op(Opcode::Return);
    asm.finish("pick", 3, 3).unwrap()
}

/// `function same(a, b) { return a === b; }`
fn strict_eq() -> Executable {
    let mut asm = Assembler::new();
    asm.get_local(0)
        .get_local(1)
        .op(Opcode::StrictEq)
        .op(Opcode::Return);
    asm.finish("strict-eq", 2, 2).unwrap()
}

#[test]
fn two_tasks_on_two_threads_return_the_identical_form_and_subtree() {
    let rt = Runtime::new();
    let (form, bindings, body) = rt.scope(|s| {
        let (form, bindings, body) = build_form(s, 1);
        // The subtree handed to a task is looked up from the original.
        let sub = s.nth(form, 1).unwrap();
        assert!(s.same_object(sub, bindings));
        (s.persist(form), s.persist(sub), s.persist(body))
    });
    let exe = pick();
    let no = rt.root(Primitive::Bool(false));
    let yes = rt.root(Primitive::Bool(true));
    let whole = rt.spawn(&exe, &[&form, &bindings, &no]).unwrap();
    let part = rt.spawn(&exe, &[&form, &bindings, &yes]).unwrap();
    // A collection between spawn and run: task frames keep the arguments.
    drop((no, yes));
    rt.collect().unwrap();

    let (got_whole, got_part) = thread::scope(|t| {
        let a = t.spawn(|| complete(whole.run().unwrap()));
        let b = t.spawn(|| complete(part.run().unwrap()));
        (a.join().unwrap(), b.join().unwrap())
    });
    drop((whole, part));
    let stats = rt.collect().unwrap();
    assert_eq!(stats.reclaimed, 0, "everything is reachable from the roots");

    // Identity, not a copy.
    assert!(got_whole.same_object(&form));
    assert!(got_part.same_object(&bindings));
    assert!(!got_whole.same_object(&got_part));
    rt.scope(|s| {
        let whole = s.local(&got_whole).unwrap();
        let part = s.local(&got_part).unwrap();
        let body_local = s.local(&body).unwrap();
        assert_eq!(s.persistent_kind(whole), Some(PersistentKind::Vector));
        assert!(s.same_object(s.nth(whole, 1).unwrap(), part));
        assert!(s.same_object(s.nth(whole, 2).unwrap(), body_local));
        let args = s.get(body_local, num(s, ARGS)).unwrap().unwrap();
        let args_via_result = s
            .get(s.nth(whole, 2).unwrap(), num(s, ARGS))
            .unwrap()
            .unwrap();
        assert!(s.same_object(args, args_via_result));

        // Contents as well as identity.
        assert_eq!(s.count(whole).unwrap(), 3);
        assert_eq!(s.count(part).unwrap(), 4);
        let leaves: Vec<(ValueView, Provenance)> = s
            .items(part)
            .unwrap()
            .into_iter()
            .map(|o| {
                let v = s.occurrence_value(o).unwrap();
                (s.view(v), s.provenance(o).unwrap())
            })
            .collect();
        assert_eq!(
            leaves,
            vec![
                (ValueView::Number(X), at(1, 7)),
                (ValueView::Number(1.0), at(1, 9)),
                (ValueView::Number(Y), at(1, 11)),
                (ValueView::Number(1.0), at(1, 13)),
            ]
        );

        // An independently built equal form is value-equal but a different
        // object; the same holds when only provenance differs.
        let (copy, copy_bindings, _) = build_form(s, 1);
        let (moved, _, _) = build_form(s, 42);
        assert!(s.value_equals(whole, copy) && !s.same_object(whole, copy));
        assert!(s.value_equals(part, copy_bindings));
        assert!(s.value_equals(whole, moved));
        assert!(!s.value_equals(whole, part));
    });
}

#[test]
fn vector_updates_keep_old_versions_and_share_untouched_subtrees_across_collection() {
    let rt = Runtime::new();
    let (v0, v1, v2, v3, weak_b, weak_x) = rt.scope(|s| {
        let a = s.new_vector(&[num(s, 1.0), num(s, 2.0)]).unwrap();
        let b = s.new_map(&[(num(s, 0.0), num(s, 1.0))]).unwrap();
        let c = s.new_object();
        let x = s.new_vector(&[num(s, 9.0)]).unwrap();
        let v0 = s.new_vector(&[a, b, c]).unwrap();
        let v1 = s.assoc(v0, num(s, 1.0), x).unwrap();
        let v2 = s.conj(v1, num(s, 5.0)).unwrap();
        let v3 = s.pop(v0).unwrap();
        // Appending through assoc at index == count.
        let appended = s.assoc(v0, num(s, 3.0), x).unwrap();
        assert_eq!(s.count(appended).unwrap(), 4);
        // An update that changes nothing returns the original object.
        let same = s.assoc(v0, num(s, 0.0), a).unwrap();
        assert!(s.same_object(same, v0));
        (
            s.persist(v0),
            s.persist(v1),
            s.persist(v2),
            s.persist(v3),
            s.downgrade(b),
            s.downgrade(x),
        )
    });
    rt.collect().unwrap();

    let check_shared = |rt: &Runtime| {
        rt.scope(|s| {
            let v1 = s.local(&v1).unwrap();
            let v2 = s.local(&v2).unwrap();
            let x = s.upgrade(&weak_x).unwrap();
            let one = s.items(v1).unwrap();
            let two = s.items(v2).unwrap();
            assert_eq!((one.len(), two.len()), (3, 4));
            assert!(s.same_object(one[1], x));
            for i in 0..3 {
                assert!(s.same_object(one[i], two[i]), "v2 shares v1[{i}]");
            }
            assert!(s.strict_equals(two[3], num(s, 5.0)));
        });
    };

    rt.scope(|s| {
        let v0 = s.local(&v0).unwrap();
        let v1 = s.local(&v1).unwrap();
        let v3 = s.local(&v3).unwrap();
        let old = s.items(v0).unwrap();
        let new = s.items(v1).unwrap();
        let b = s.upgrade(&weak_b).unwrap();
        // The prior version is unchanged...
        assert_eq!(old.len(), 3);
        assert!(s.same_object(old[1], b));
        // ...and the untouched elements are the same objects in both.
        assert!(s.same_object(old[0], new[0]));
        assert!(s.same_object(old[2], new[2]));
        assert!(!s.same_object(old[1], new[1]));
        let popped = s.items(v3).unwrap();
        assert_eq!(popped.len(), 2);
        assert!(s.same_object(popped[0], old[0]) && s.same_object(popped[1], old[1]));
        assert!(!s.same_object(v0, v1));
    });
    check_shared(&rt);

    // Only v0 and v3 reference `b`: dropping them reclaims v0, v3 and b, and
    // nothing that v1/v2 still share.
    drop((v0, v3));
    let stats = rt.collect().unwrap();
    assert_eq!(stats.reclaimed, 3);
    assert!(!weak_b.is_live());
    assert!(weak_x.is_live());
    check_shared(&rt);
}

#[test]
fn map_updates_keep_old_versions_and_share_values_across_collection() {
    let rt = Runtime::new();
    let (m0, m1, m2, m3) = rt.scope(|s| {
        let a = s.new_vector(&[]).unwrap();
        let b = s.new_object();
        let c = s.new_object();
        let d = s.new_object();
        let m0 = s
            .new_map(&[(num(s, 1.0), a), (num(s, 2.0), b), (a, c)])
            .unwrap();
        let m1 = s.assoc(m0, num(s, 3.0), c).unwrap();
        let m2 = s.dissoc(m1, num(s, 1.0)).unwrap();
        let m3 = s.assoc(m0, num(s, 2.0), d).unwrap();
        // No-op updates return the original object.
        assert!(s.same_object(s.dissoc(m0, num(s, 99.0)).unwrap(), m0));
        assert!(s.same_object(s.assoc(m0, num(s, 2.0), b).unwrap(), m0));
        (s.persist(m0), s.persist(m1), s.persist(m2), s.persist(m3))
    });
    for round in 0..2 {
        // Round 1 reclaims only the probe vector round 0 allocated.
        assert_eq!(rt.collect().unwrap().reclaimed, round);
        rt.scope(|s| {
            let [m0, m1, m2, m3] = [&m0, &m1, &m2, &m3].map(|m| s.local(m).unwrap());
            let get = |m, k: f64| s.get(m, num(s, k)).unwrap();
            assert_eq!([m0, m1, m2, m3].map(|m| s.count(m).unwrap()), [3, 4, 3, 3]);
            // m0 is unchanged by every later version.
            let a = get(m0, 1.0).unwrap();
            let b = get(m0, 2.0).unwrap();
            assert!(get(m0, 3.0).is_none());
            // Keys that are objects compare by identity.
            let c = s.get(m0, a).unwrap().unwrap();
            let equal_but_distinct = s.new_vector(&[]).unwrap();
            assert!(s.value_equals(a, equal_but_distinct));
            assert!(s.get(m0, equal_but_distinct).unwrap().is_none());
            // Shared values keep their identity in every version.
            assert!(s.same_object(get(m1, 1.0).unwrap(), a));
            assert!(s.same_object(get(m1, 2.0).unwrap(), b));
            assert!(s.same_object(get(m1, 3.0).unwrap(), c));
            assert!(get(m2, 1.0).is_none());
            assert!(s.same_object(get(m2, 3.0).unwrap(), c));
            assert!(!s.same_object(get(m3, 2.0).unwrap(), b));
            assert!(s.same_object(get(m3, 1.0).unwrap(), a));
            assert_eq!(s.entries(m2).unwrap().len(), 3);
        });
    }
}

#[test]
fn equal_scalar_occurrences_keep_distinct_provenance() {
    let rt = Runtime::new();
    let (first, second) = rt.scope(|s| {
        let first = occ(s, 1.0, at(3, 9));
        let second = occ(s, 1.0, at(3, 13));
        // Value-equal (to each other and to the bare scalar)...
        assert!(s.value_equals(first, second));
        assert!(s.value_equals(first, num(s, 1.0)));
        assert!(!s.value_equals(first, num(s, 2.0)));
        // ...but two objects with two provenances.
        assert!(!s.same_object(first, second));
        assert_eq!(s.provenance(first).unwrap(), at(3, 9));
        assert_eq!(s.provenance(second).unwrap(), at(3, 13));
        let (v1, v2) = (
            s.occurrence_value(first).unwrap(),
            s.occurrence_value(second).unwrap(),
        );
        assert!(s.strict_equals(v1, v2));
        // Collections of them compare equal as values.
        let xs = s.new_vector(&[first]).unwrap();
        let ys = s.new_vector(&[second]).unwrap();
        assert!(s.value_equals(xs, ys));
        assert_eq!(s.persistent_kind(first), Some(PersistentKind::Occurrence));
        (s.persist(first), s.persist(second))
    });
    // VM `===` is identity: the two occurrences are not the same object.
    let same = complete(
        rt.spawn(&strict_eq(), &[&first, &second])
            .unwrap()
            .run()
            .unwrap(),
    );
    assert_eq!(same.view(), ValueView::Bool(false));
    let itself = complete(
        rt.spawn(&strict_eq(), &[&first, &first])
            .unwrap()
            .run()
            .unwrap(),
    );
    assert_eq!(itself.view(), ValueView::Bool(true));
}

#[test]
fn the_error_contract_is_reported_without_panicking() {
    let rt = Runtime::new();
    rt.scope(|s| {
        let vec = s.new_vector(&[num(s, 10.0), num(s, 20.0)]).unwrap();
        let map = s.new_map(&[(num(s, 1.0), num(s, 2.0))]).unwrap();
        let empty = s.new_vector(&[]).unwrap();
        let plain = s.new_object();
        let five = num(s, 5.0);
        let wrong = |expected, found| CollectionError::WrongType { expected, found };

        assert_eq!(s.count(five), Err(wrong("vector or map", "number")));
        assert_eq!(s.count(plain), Err(wrong("vector or map", "object")));
        assert_eq!(s.nth(map, 0).unwrap_err(), wrong("vector", "map"));
        assert_eq!(s.dissoc(vec, five).unwrap_err(), wrong("map", "vector"));
        assert_eq!(s.conj(map, five).unwrap_err(), wrong("vector", "map"));
        assert_eq!(s.provenance(vec), Err(wrong("occurrence", "vector")));
        assert_eq!(s.persistent_kind(plain), None);

        assert_eq!(
            s.nth(vec, 2).unwrap_err(),
            CollectionError::IndexOutOfBounds { index: 2, count: 2 }
        );
        assert_eq!(
            s.assoc(vec, num(s, 3.0), five).unwrap_err(),
            CollectionError::IndexOutOfBounds { index: 3, count: 2 }
        );
        assert_eq!(
            s.pop(empty).unwrap_err(),
            CollectionError::IndexOutOfBounds { index: 0, count: 0 }
        );
        for bad in [
            num(s, -1.0),
            num(s, 0.5),
            num(s, f64::NAN),
            s.primitive(Primitive::Null),
            plain,
        ] {
            assert_eq!(
                s.assoc(vec, bad, five).unwrap_err(),
                CollectionError::InvalidIndex
            );
            assert_eq!(s.get(vec, bad).unwrap_err(), CollectionError::InvalidIndex);
        }
        // Absence is not an error for `get`.
        assert!(s.get(vec, num(s, 2.0)).unwrap().is_none());
        assert!(s.get(map, num(s, 2.0)).unwrap().is_none());
        assert!(s.strict_equals(s.get(vec, num(s, -0.0)).unwrap().unwrap(), num(s, 10.0)));
        // Nothing failed changed the inputs.
        assert_eq!(s.count(vec), Ok(2));
        assert_eq!(s.count(map), Ok(1));
    });
}

#[test]
fn a_rooted_collection_keeps_its_whole_graph_alive_until_released() {
    let rt = Runtime::new();
    let (root, weak_leaf) = rt.scope(|s| {
        let leaf = s.new_object();
        let inner = s.new_map(&[(leaf, leaf)]).unwrap();
        let occ = s.new_occurrence(inner, at(1, 1)).unwrap();
        let outer = s.new_vector(&[occ, occ]).unwrap();
        (s.persist(outer), s.downgrade(leaf))
    });
    let stats = rt.collect().unwrap();
    assert_eq!((stats.live, stats.reclaimed), (4, 0));
    assert!(weak_leaf.is_live());
    drop(root);
    let stats = rt.collect().unwrap();
    assert_eq!((stats.live, stats.reclaimed), (0, 4));
    assert!(!weak_leaf.is_live());
}

#[test]
fn structural_equality_rejects_same_count_differences() {
    let rt = Runtime::new();
    rt.scope(|s| {
        let one = s.new_vector(&[num(s, 1.0)]).unwrap();
        let two = s.new_vector(&[num(s, 2.0)]).unwrap();
        assert!(!s.value_equals(one, two));
        let k1 = s.new_map(&[(num(s, 1.0), num(s, 1.0))]).unwrap();
        let k2 = s.new_map(&[(num(s, 2.0), num(s, 1.0))]).unwrap();
        let v2 = s.new_map(&[(num(s, 1.0), num(s, 2.0))]).unwrap();
        assert!(!s.value_equals(k1, k2));
        assert!(!s.value_equals(k1, v2));
        // A nested difference behind occurrence wrappers.
        let deep = |x: f64, col| {
            let leaf = occ(s, x, at(1, col));
            let inner = s.new_vector(&[num(s, 0.0), leaf]).unwrap();
            let wrapped = s.new_occurrence(inner, at(1, 1)).unwrap();
            s.new_map(&[(num(s, 7.0), wrapped)]).unwrap()
        };
        assert!(s.value_equals(deep(3.0, 5), deep(3.0, 9)));
        assert!(!s.value_equals(deep(3.0, 5), deep(4.0, 5)));
        // Different kinds with equal counts.
        let vec0 = s.new_vector(&[]).unwrap();
        let map0 = s.new_map(&[]).unwrap();
        assert!(!s.value_equals(vec0, map0));
    });
}

fn bits<'s>(s: &Scope<'s>, v: Local<'s>) -> u64 {
    match s.view(v) {
        ValueView::Number(x) => x.to_bits(),
        other => panic!("not a number: {other:?}"),
    }
}

#[test]
fn numeric_updates_preserve_bits_and_no_op_identity() {
    let nan_a = f64::from_bits(0x7ff8_0000_0000_0001);
    let nan_b = f64::from_bits(0x7ff8_0000_0000_0002);
    let rt = Runtime::new();
    rt.scope(|s| {
        let vec = s.new_vector(&[num(s, nan_a), num(s, 0.0)]).unwrap();
        let map = s
            .new_map(&[(num(s, 1.0), num(s, nan_a)), (num(s, 2.0), num(s, 0.0))])
            .unwrap();
        for (coll, nan_key, zero_key) in [(vec, 0.0, 1.0), (map, 1.0, 2.0)] {
            // The identical NaN payload is a no-op: the original object.
            let same = s.assoc(coll, num(s, nan_key), num(s, nan_a)).unwrap();
            assert!(s.same_object(same, coll));
            // A different NaN payload is a change.
            let other = s.assoc(coll, num(s, nan_key), num(s, nan_b)).unwrap();
            assert!(!s.same_object(other, coll));
            let got = s.get(other, num(s, nan_key)).unwrap().unwrap();
            assert_eq!(bits(s, got), nan_b.to_bits());
            // -0 replaces +0 with the requested bits; the old version keeps +0.
            let neg = s.assoc(coll, num(s, zero_key), num(s, -0.0)).unwrap();
            assert!(!s.same_object(neg, coll));
            let got = s.get(neg, num(s, zero_key)).unwrap().unwrap();
            assert_eq!(bits(s, got), (-0.0f64).to_bits());
            let old = s.get(coll, num(s, zero_key)).unwrap().unwrap();
            assert_eq!(bits(s, old), 0.0f64.to_bits());
            let old_nan = s.get(coll, num(s, nan_key)).unwrap().unwrap();
            assert_eq!(bits(s, old_nan), nan_a.to_bits());
        }
    });
}

#[test]
fn map_construction_keeps_the_last_duplicate_same_value_zero_key() {
    let nan_a = f64::from_bits(0x7ff8_0000_0000_0001);
    let nan_b = f64::from_bits(0xfff8_0000_0000_0002);
    let rt = Runtime::new();
    rt.scope(|s| {
        let map = s
            .new_map(&[
                (num(s, 5.0), num(s, 1.0)),
                (num(s, -0.0), num(s, 2.0)),
                (num(s, nan_a), num(s, 3.0)),
                (num(s, 5.0), num(s, 4.0)),
                (num(s, 0.0), num(s, 5.0)),
                (num(s, nan_b), num(s, 6.0)),
            ])
            .unwrap();
        assert_eq!(s.count(map), Ok(3));
        let get = |k: f64| s.view(s.get(map, num(s, k)).unwrap().unwrap());
        assert_eq!(get(5.0), ValueView::Number(4.0));
        assert_eq!(get(-0.0), ValueView::Number(5.0));
        assert_eq!(get(0.0), ValueView::Number(5.0));
        assert_eq!(get(f64::NAN), ValueView::Number(6.0));
        // The zero key is stored canonically as +0.
        let zero_keys: Vec<u64> = s
            .entries(map)
            .unwrap()
            .into_iter()
            .filter(|(k, _)| matches!(s.view(*k), ValueView::Number(z) if z == 0.0))
            .map(|(k, _)| bits(s, k))
            .collect();
        assert_eq!(zero_keys, vec![0.0f64.to_bits()]);
    });
}
