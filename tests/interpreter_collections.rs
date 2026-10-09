use light_cljs::interpreter::{
    CollectionError, Context, Name, NumberError, RootedValue, ValueView,
};
#[path = "support/sci_protocol.rs"]
#[allow(dead_code)]
mod protocol;

fn keyword(c: &Context, text: &str) -> RootedValue {
    c.keyword(Name::new(None, text))
}
fn text(c: &Context, text: &str) -> RootedValue {
    c.string(text.encode_utf16().collect::<Vec<_>>())
}
fn ints(c: &Context, values: &[i64]) -> Vec<RootedValue> {
    values.iter().map(|v| c.integer(*v)).collect()
}
fn integer(value: &RootedValue) -> i64 {
    let ValueView::Integer(v) = value.inspect() else {
        panic!("integer")
    };
    v
}
fn equal(a: &RootedValue, b: &RootedValue) {
    assert!(a.value_eq(b), "{a:?} != {b:?}");
    assert!(b.value_eq(a), "asymmetric equality");
    assert_eq!(a.value_hash(), b.value_hash());
}
fn wire_value(value: &RootedValue) -> protocol::Value {
    use protocol::Value as V;
    match value.inspect() {
        ValueView::LazySeq | ValueView::Delay => {
            panic!("deferred values must be projected by the evaluated fixture")
        }
        ValueView::NativeFunction(_)
        | ValueView::Function
        | ValueView::Var(_)
        | ValueView::Namespace(_)
        | ValueView::Unbound(_)
        | ValueView::Exception(_)
        | ValueView::Atom
        | ValueView::Input
        | ValueView::Output
        | ValueView::Volatile
        | ValueView::Reduced => {
            panic!("callables are outside fixture transport")
        }
        ValueView::Nil => V::Nil,
        ValueView::Boolean(v) => V::Bool(v),
        ValueView::Integer(v) => V::Int(v),
        ValueView::Double(v) => V::Float(v.to_bits()),
        ValueView::Ratio {
            numerator,
            denominator,
        } => V::Ratio(numerator, denominator),
        ValueView::Character(v) => V::Char(v),
        ValueView::String(v) => V::String(v.to_vec()),
        ValueView::Symbol(v) => V::Symbol(v.namespace().map(<[u16]>::to_vec), v.name().to_vec()),
        ValueView::Keyword(v) => V::Keyword(v.namespace().map(<[u16]>::to_vec), v.name().to_vec()),
        ValueView::List(_) => V::List(value.elements().unwrap().iter().map(wire_value).collect()),
        ValueView::Seq(_) => V::Seq(value.elements().unwrap().iter().map(wire_value).collect()),
        ValueView::Vector(_) => {
            V::Vector(value.elements().unwrap().iter().map(wire_value).collect())
        }
        ValueView::Map(_) => {
            let mut pairs: Vec<_> = value
                .entries()
                .unwrap()
                .iter()
                .map(|(k, v)| (wire_value(k), wire_value(v)))
                .collect();
            pairs.sort_by_key(|(k, v)| format!("(entry {} {})", k.wire(), v.wire()));
            V::Map(pairs)
        }
        ValueView::Set(_) => {
            let mut values: Vec<_> = value.elements().unwrap().iter().map(wire_value).collect();
            values.sort_by_key(V::wire);
            V::Set(values)
        }
    }
}
fn reference(id: &str, actual: protocol::Value) {
    let cases = protocol::cases(include_str!("fixtures/sci-outcomes.tsv")).unwrap();
    let case = cases.iter().find(|c| c.id == id).unwrap();
    assert_eq!(case.outcome, protocol::Outcome::Returned(actual), "{id}");
}

#[test]
fn nested_equality_and_hashes_match_pinned_sci_observations() {
    // Execute equivalent native storage operations, not the Clojure fixture source.
    let c = Context::new();
    let a = keyword(&c, "a");
    let set = c.set(&ints(&c, &[2, 3])).unwrap();
    let reverse = c.set(&ints(&c, &[3, 2])).unwrap();
    let map = c.map(&[(a.clone(), set)]).unwrap();
    let other_map = c.map(&[(a, reverse)]).unwrap();
    let vector = c.vector(&[c.integer(1), map]).unwrap();
    let list = c.list(&[c.integer(1), other_map]).unwrap();
    let results = [
        vector.value_eq(&list),
        vector.value_hash() == list.value_hash(),
        c.vector(&[]).unwrap().value_eq(&c.list(&[]).unwrap()),
        c.map(&[]).unwrap().value_eq(&c.set(&[]).unwrap()),
        c.integer(1).value_eq(&c.double(1.0)),
        c.ratio(1, 2).unwrap().value_eq(&c.double(0.5)),
        c.ratio(2, 4).unwrap().value_eq(&c.ratio(1, 2).unwrap()),
        c.double(0.0).value_eq(&c.double(-0.0)),
        c.double(0.0).value_hash() == c.double(-0.0).value_hash(),
    ];
    reference(
        "sci-collection-equality",
        protocol::Value::Vector(results.into_iter().map(protocol::Value::Bool).collect()),
    );
    equal(&vector, &list);
    let foreign = Context::new();
    let foreign_map = foreign
        .map(&[(
            keyword(&foreign, "a"),
            foreign.set(&ints(&foreign, &[3, 2])).unwrap(),
        )])
        .unwrap();
    equal(
        &vector,
        &foreign.list(&[foreign.integer(1), foreign_map]).unwrap(),
    );
    assert!(!c.nil().value_eq(&c.boolean(false)));
    assert!(!c.vector(&[]).unwrap().value_eq(&c.nil()));
    assert!(!c.symbol(Name::new(None, "a")).value_eq(&keyword(&c, "a")));
}

#[test]
fn equal_keys_missing_values_and_numeric_categories_match_sci() {
    let c = Context::new();
    let key = c.vector(&ints(&c, &[1, 2])).unwrap();
    let list_key = c.list(&ints(&c, &[1, 2])).unwrap();
    let original = c
        .map(&[
            (key.clone(), c.nil()),
            (c.nil(), c.boolean(false)),
            (c.integer(1), keyword(&c, "integer")),
            (c.double(1.0), keyword(&c, "double")),
        ])
        .unwrap();
    assert!(original.contains(&key).unwrap());
    assert_eq!(
        original.get(&key).unwrap().unwrap().inspect(),
        ValueView::Nil
    );
    let updated = c
        .assoc(&original, &list_key, &keyword(&c, "updated"))
        .unwrap();
    let actual = c
        .vector(&[
            c.integer(updated.count().unwrap() as i64),
            updated.get(&key).unwrap().unwrap(),
            updated.get(&c.nil()).unwrap().unwrap(),
            c.boolean(updated.contains(&c.nil()).unwrap()),
            c.boolean(updated.contains(&keyword(&c, "missing")).unwrap()),
            updated.get(&c.integer(1)).unwrap().unwrap(),
            updated.get(&c.double(1.0)).unwrap().unwrap(),
        ])
        .unwrap();
    reference("sci-collection-keys", wire_value(&actual));
    assert_eq!(
        original.get(&key).unwrap().unwrap().inspect(),
        ValueView::Nil
    );
    let (retained, _) = updated
        .entries()
        .unwrap()
        .into_iter()
        .find(|(k, _)| k.value_eq(&key))
        .unwrap();
    assert!(retained.same_identity(&key));
    let removed = c.dissoc(&updated, &list_key).unwrap();
    assert!(!removed.contains(&key).unwrap());
    assert!(updated.contains(&key).unwrap());
    let set = c
        .set(&[key.clone(), list_key, c.nil(), c.boolean(false)])
        .unwrap();
    assert_eq!(set.count(), Some(3));
    assert!(set.get(&key).unwrap().unwrap().same_identity(&key));
    assert!(set.contains(&c.nil()).unwrap());
    assert!(set.contains(&c.boolean(false)).unwrap());
    assert_eq!(c.disj(&set, &c.nil()).unwrap().count(), Some(2));
}

#[test]
fn real_hash_collisions_use_equality_and_preserve_old_versions() {
    let c = Context::new();
    let a = text(&c, "Aa");
    let b = text(&c, "BB");
    assert!(!a.value_eq(&b));
    assert_eq!(a.value_hash(), b.value_hash());
    let original = c
        .map(&[(a.clone(), c.integer(1)), (b.clone(), c.integer(2))])
        .unwrap();
    let updated = c.assoc(&original, &a, &c.integer(3)).unwrap();
    let removed = c.dissoc(&updated, &b).unwrap();
    assert_eq!(integer(&original.get(&a).unwrap().unwrap()), 1);
    assert_eq!(integer(&updated.get(&a).unwrap().unwrap()), 3);
    assert_eq!(integer(&removed.get(&a).unwrap().unwrap()), 3);
    assert!(removed.get(&b).unwrap().is_none());
    let reverse = c
        .map(&[(b.clone(), c.integer(2)), (a.clone(), c.integer(1))])
        .unwrap();
    equal(&original, &reverse);
    assert!(
        !original.value_eq(
            &c.map(&[(a.clone(), c.integer(2)), (b.clone(), c.integer(1))])
                .unwrap()
        )
    );
    let set = c.set(&[a.clone(), b.clone(), a.clone()]).unwrap();
    equal(&set, &c.set(&[b.clone(), a.clone()]).unwrap());
    assert_eq!(set.count(), Some(2));
    assert_eq!(c.disj(&set, &a).unwrap().count(), Some(1));
    // Collisions survive nesting and sequential key equivalence.
    let ka = c.vector(&[a]).unwrap();
    let kb = c.list(&[b]).unwrap();
    assert_eq!(ka.value_hash(), kb.value_hash());
    let nested = c
        .map(&[(ka.clone(), c.integer(1)), (kb.clone(), c.integer(2))])
        .unwrap();
    assert_eq!(integer(&nested.get(&kb).unwrap().unwrap()), 2);
    assert_eq!(integer(&nested.get(&ka).unwrap().unwrap()), 1);
}

#[test]
fn persistent_vector_boundaries_and_list_order_survive_updates() {
    let mut c = Context::new();
    let mut vector = c.vector(&[]).unwrap();
    let mut retained = Vec::new();
    for index in 0..33_000 {
        vector = c.conj(&vector, &c.integer(index)).unwrap();
        if [1, 31, 32, 33, 1023, 1024, 1025, 32767, 32768, 32769].contains(&(index + 1)) {
            retained.push(vector.clone());
        }
        if index % 500 == 0 {
            c.collect_garbage();
        }
    }
    let changed = c.assoc(&vector, &c.integer(1024), &c.integer(-1)).unwrap();
    assert_eq!(integer(&changed.nth(1024).unwrap().unwrap()), -1);
    assert_eq!(integer(&vector.nth(1024).unwrap().unwrap()), 1024);
    for old in retained {
        let n = old.count().unwrap();
        assert_eq!(integer(&old.nth(n - 1).unwrap().unwrap()), n as i64 - 1);
        assert!(old.nth(n).unwrap().is_none());
    }
    for n in (0..33_000).rev() {
        vector = c.pop(&vector).unwrap();
        assert_eq!(vector.count(), Some(n));
        if n > 0 {
            assert_eq!(integer(&vector.nth(n - 1).unwrap().unwrap()), n as i64 - 1);
        }
        if n % 500 == 0 {
            c.collect_garbage();
        }
    }
    assert_eq!(
        c.pop(&vector).unwrap_err(),
        CollectionError::EmptyCollection
    );
    let list = c.list(&ints(&c, &[1, 2, 3])).unwrap();
    let prepended = c.conj(&list, &c.integer(0)).unwrap();
    equal(&c.pop(&prepended).unwrap(), &list);
    assert!(c.pop(&prepended).unwrap().same_identity(&list));
    assert_eq!(
        prepended
            .elements()
            .unwrap()
            .iter()
            .map(integer)
            .collect::<Vec<_>>(),
        [0, 1, 2, 3]
    );
    let entry = c.vector(&[c.integer(1), c.integer(9)]).unwrap();
    assert_eq!(
        integer(
            &c.conj(&c.map(&[]).unwrap(), &entry)
                .unwrap()
                .get(&c.integer(1))
                .unwrap()
                .unwrap()
        ),
        9
    );
}

#[test]
fn metadata_replacement_nested_maps_and_list_tails_match_sci() {
    // Adapt SCI core_test.cljc meta-test (1610-1641), pin in NOTICE.md.
    let c = Context::new();
    let original = c.vector(&[]).unwrap();
    let metadata = c
        .map(&[
            (keyword(&c, "line"), c.integer(2)),
            (keyword(&c, "column"), c.integer(14)),
        ])
        .unwrap();
    let annotated = c.with_meta(&original, &metadata).unwrap();
    reference(
        "sci-meta-replacement",
        wire_value(&annotated.metadata().unwrap()),
    );
    assert!(original.metadata().is_none());
    equal(&original, &annotated);
    let n = c.map(&[(keyword(&c, "n"), c.integer(6))]).unwrap();
    let nested = c.with_meta(&c.map(&[]).unwrap(), &n).unwrap();
    let m = c.map(&[(keyword(&c, "m"), nested)]).unwrap();
    let outer = c
        .with_meta(
            &c.map(&[(keyword(&c, "foo"), c.map(&[]).unwrap())]).unwrap(),
            &m,
        )
        .unwrap();
    reference(
        "sci-meta-nested",
        wire_value(
            &outer
                .metadata()
                .unwrap()
                .get(&keyword(&c, "m"))
                .unwrap()
                .unwrap()
                .metadata()
                .unwrap(),
        ),
    );
    let tail_meta = c.map(&[(keyword(&c, "tag"), keyword(&c, "tail"))]).unwrap();
    let outer_meta = c
        .map(&[(keyword(&c, "tag"), keyword(&c, "outer"))])
        .unwrap();
    let tail = c
        .with_meta(&c.list(&ints(&c, &[2])).unwrap(), &tail_meta)
        .unwrap();
    let xs = c
        .with_meta(&c.conj(&tail, &c.integer(1)).unwrap(), &outer_meta)
        .unwrap();
    let bare_tail = c
        .pop(
            &c.with_meta(&c.list(&ints(&c, &[1, 2])).unwrap(), &outer_meta)
                .unwrap(),
        )
        .unwrap();
    let singleton = c
        .pop(
            &c.with_meta(&c.list(&ints(&c, &[1])).unwrap(), &outer_meta)
                .unwrap(),
        )
        .unwrap();
    let actual = c
        .vector(&[
            c.pop(&xs).unwrap().metadata().unwrap(),
            bare_tail.metadata().unwrap_or_else(|| c.nil()),
            singleton.metadata().unwrap(),
        ])
        .unwrap();
    reference("sci-meta-list-tail", wire_value(&actual));
    let symbol = c.symbol(Name::new(None, "x"));
    let named = c.with_meta(&symbol, &metadata).unwrap();
    equal(&symbol, &named);
    assert_eq!(symbol.symbol_id(), named.symbol_id());
    equal(&c.with_meta(&named, &c.nil()).unwrap(), &symbol);
    for collection in [
        c.vector(&ints(&c, &[1])).unwrap(),
        c.map(&[(c.integer(1), c.integer(2))]).unwrap(),
        c.set(&ints(&c, &[1])).unwrap(),
    ] {
        let marked = c.with_meta(&collection, &metadata).unwrap();
        let updated = match marked.inspect() {
            ValueView::Vector(_) => c.assoc(&marked, &c.integer(0), &c.integer(2)).unwrap(),
            ValueView::Map(_) => c.dissoc(&marked, &c.integer(1)).unwrap(),
            _ => c.disj(&marked, &c.integer(1)).unwrap(),
        };
        equal(&updated.metadata().unwrap(), &metadata);
        equal(&marked, &collection);
    }
    let first = c
        .with_meta(&c.vector(&ints(&c, &[1])).unwrap(), &metadata)
        .unwrap();
    let second = c
        .with_meta(&c.list(&ints(&c, &[1])).unwrap(), &outer_meta)
        .unwrap();
    let duplicate = c
        .map(&[
            (first.clone(), c.integer(1)),
            (second.clone(), c.integer(2)),
        ])
        .unwrap();
    assert!(duplicate.entries().unwrap()[0].0.same_identity(&first));
    assert!(
        c.set(&[first.clone(), second])
            .unwrap()
            .get(&first)
            .unwrap()
            .unwrap()
            .same_identity(&first)
    );
}

#[test]
fn map_content_adapts_sci_array_based_map_assertion_without_iteration_assumptions() {
    // Adapt SCI array-based-map-test (1667-1671): compare associations from the
    // eight-entry literal; its array-map traversal representation is excluded.
    let c = Context::new();
    let entries: Vec<_> = (0..8).map(|i| (c.integer(i), c.integer(i))).collect();
    let map = c.map(&entries).unwrap();
    let mut rebuilt = c.map(&[]).unwrap();
    for (k, v) in entries.iter().rev() {
        rebuilt = c.assoc(&rebuilt, k, v).unwrap();
    }
    reference(
        "sci-map-eight-entries",
        protocol::Value::Bool(map.value_eq(&rebuilt)),
    );
    equal(&map, &rebuilt);
    for (k, v) in entries {
        equal(&map.get(&k).unwrap().unwrap(), &v);
    }
}

#[test]
fn ownership_validation_and_metadata_rejection_allocate_nothing() {
    let c = Context::new();
    let foreign = Context::new();
    let value = c.vector(&[]).unwrap();
    let scalar = foreign.integer(1);
    let local = c.integer(1);
    let before = c.memory_stats();
    assert_eq!(
        c.vector(std::slice::from_ref(&scalar)).unwrap_err(),
        CollectionError::WrongOwner
    );
    assert_eq!(
        c.list(std::slice::from_ref(&scalar)).unwrap_err(),
        CollectionError::WrongOwner
    );
    assert_eq!(
        c.set(std::slice::from_ref(&scalar)).unwrap_err(),
        CollectionError::WrongOwner
    );
    assert_eq!(
        c.map(&[(local.clone(), scalar.clone())]).unwrap_err(),
        CollectionError::WrongOwner
    );
    assert_eq!(
        c.conj(&value, &scalar).unwrap_err(),
        CollectionError::WrongOwner
    );
    assert_eq!(
        c.assoc(&value, &local, &scalar).unwrap_err(),
        CollectionError::WrongOwner
    );
    assert_eq!(
        c.with_meta(&value, &scalar).unwrap_err(),
        CollectionError::WrongOwner
    );
    assert_eq!(value.get(&scalar).unwrap_err(), CollectionError::WrongOwner);
    assert_eq!(c.memory_stats(), before);
    for target in [
        c.nil(),
        c.boolean(false),
        local.clone(),
        c.double(1.0),
        c.ratio(1, 2).unwrap(),
        text(&c, "x"),
        keyword(&c, "x"),
        c.character(97),
    ] {
        assert_eq!(
            c.with_meta(&target, &c.nil()).unwrap_err(),
            CollectionError::UnsupportedMetadata
        );
    }
    assert_eq!(
        c.with_meta(&value, &local).unwrap_err(),
        CollectionError::ExpectedMetadataMap
    );
    assert_eq!(
        c.assoc(&value, &c.integer(-1), &local).unwrap_err(),
        CollectionError::InvalidIndex
    );
    assert_eq!(
        c.assoc(&value, &c.integer(1), &local).unwrap_err(),
        CollectionError::InvalidIndex
    );
    assert_eq!(
        c.assoc(&value, &c.double(0.0), &local).unwrap_err(),
        CollectionError::InvalidIndex
    );
    assert_eq!(
        c.assoc(&value, &c.integer(0), &local).unwrap().count(),
        Some(1)
    );
}

#[test]
fn canonical_ratios_signed_zero_and_nan_contract_are_explicit() {
    let c = Context::new();
    equal(&c.ratio(2, -4).unwrap(), &c.ratio(-1, 2).unwrap());
    equal(&c.ratio(0, i64::MIN).unwrap(), &c.integer(0));
    equal(&c.ratio(i64::MIN, i64::MIN).unwrap(), &c.integer(1));
    equal(&c.ratio(i64::MIN, -2).unwrap(), &c.integer(1i64 << 62));
    assert_eq!(c.ratio(1, 0).unwrap_err(), NumberError::ZeroDenominator);
    assert_eq!(c.ratio(1, i64::MIN).unwrap_err(), NumberError::Overflow);
    assert_eq!(c.ratio(i64::MIN, -1).unwrap_err(), NumberError::Overflow);
    let zero = c.double(-0.0);
    let positive = c.double(0.0);
    equal(&zero, &positive);
    let set = c.set(&[zero.clone(), positive]).unwrap();
    assert_eq!(set.count(), Some(1));
    assert!(set.get(&zero).unwrap().unwrap().same_identity(&zero));
    let nan = c.double(f64::NAN);
    assert!(!nan.value_eq(&nan));
    let map = c.map(&[(nan.clone(), c.integer(1))]).unwrap();
    assert!(!map.contains(&nan).unwrap());
    let a = c.vector(std::slice::from_ref(&nan)).unwrap();
    assert!(a.value_eq(&a));
    assert!(!a.value_eq(&c.vector(&[nan]).unwrap()));
    reference(
        "sci-ratios",
        wire_value(
            &c.vector(&[
                c.ratio(2, 4).unwrap(),
                c.ratio(-4, 6).unwrap(),
                c.ratio(4, 2).unwrap(),
            ])
            .unwrap(),
        ),
    );
}

#[test]
fn shared_nested_values_metadata_and_discarded_versions_are_reclaimed() {
    let mut c = Context::new();
    let child = c.string(vec![0xd800]);
    let child_id = child.heap_id().unwrap();
    let nested = c.vector(std::slice::from_ref(&child)).unwrap();
    let retained = c.vector(&[nested.clone(), nested]).unwrap();
    drop(child);
    c.collect_garbage();
    assert_eq!(
        c.heap_value(child_id).unwrap().inspect(),
        ValueView::String(vec![0xd800].into())
    );
    let before = c.memory_stats().heap_values;
    let mut current = retained.clone();
    for i in 0..1_000 {
        let metadata = c
            .map(&[(keyword(&c, &format!("temporary-{i}")), c.integer(i))])
            .unwrap();
        current = c.with_meta(&current, &metadata).unwrap();
        if i % 10 == 0 {
            c.collect_garbage();
        }
    }
    let stats = c.collect_garbage().after;
    assert_eq!(stats.heap_values, before + 2);
    assert_eq!(stats.names, 1);
    let extracted = current.nth(0).unwrap().unwrap();
    drop((retained, current));
    c.collect_garbage();
    assert_eq!(c.memory_stats().names, 0);
    assert_eq!(extracted.nth(0).unwrap().unwrap().heap_id(), Some(child_id));
    c.reset();
    assert_eq!(
        extracted.nth(0).unwrap().unwrap().inspect(),
        ValueView::String(vec![0xd800].into())
    );
    assert_eq!(
        c.vector(&[extracted]).unwrap_err(),
        CollectionError::WrongOwner
    );
}

#[test]
fn deep_language_nesting_and_shared_dags_do_not_recurse_on_the_rust_stack() {
    let mut c = Context::new();
    let mut left = c.integer(0);
    let mut right = c.integer(0);
    for _ in 0..20_000 {
        left = c.vector(&[left]).unwrap();
        right = c.list(&[right]).unwrap();
    }
    equal(&left, &right);
    c.collect_garbage();
    drop((left, right));
    assert_eq!(c.collect_garbage().after.heap_values, 0);
    let mut left = c.integer(1);
    let mut right = c.integer(1);
    for _ in 0..30 {
        left = c.vector(&[left.clone(), left]).unwrap();
        right = c.vector(&[right.clone(), right]).unwrap();
    }
    // Expanded traversal would visit more than a billion elements.
    equal(&left, &right);
}

#[test]
fn map_and_set_updates_match_independent_models_across_many_hash_paths() {
    use std::collections::{BTreeMap, BTreeSet};
    let mut c = Context::new();
    let mut map = c.map(&[]).unwrap();
    let mut set = c.set(&[]).unwrap();
    let mut expected = BTreeMap::new();
    let mut members = BTreeSet::new();
    let mut snapshots = Vec::new();
    let mut seed = 17u64;
    for step in 0..2_000 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let key = ((seed >> 32) % 256) as i64;
        let rooted = c.integer(key);
        if seed & 3 == 0 {
            map = c.dissoc(&map, &rooted).unwrap();
            set = c.disj(&set, &rooted).unwrap();
            expected.remove(&key);
            members.remove(&key);
        } else {
            map = c.assoc(&map, &rooted, &c.integer(step)).unwrap();
            set = c.conj(&set, &rooted).unwrap();
            expected.insert(key, step);
            members.insert(key);
        }
        assert_eq!(map.count(), Some(expected.len()));
        assert_eq!(set.count(), Some(members.len()));
        if step % 199 == 0 {
            snapshots.push((map.clone(), set.clone(), expected.clone(), members.clone()));
            c.collect_garbage();
        }
    }
    snapshots.push((map, set, expected, members));
    for (map, set, expected, members) in snapshots {
        for key in 0..256 {
            assert_eq!(
                map.get(&c.integer(key)).unwrap().as_ref().map(integer),
                expected.get(&key).copied()
            );
            assert_eq!(
                set.contains(&c.integer(key)).unwrap(),
                members.contains(&key)
            );
        }
        let pairs: Vec<_> = expected
            .iter()
            .rev()
            .map(|(k, v)| (c.integer(*k), c.integer(*v)))
            .collect();
        equal(&map, &c.map(&pairs).unwrap());
        let values: Vec<_> = members.iter().rev().map(|v| c.integer(*v)).collect();
        equal(&set, &c.set(&values).unwrap());
    }
}
