//! Native Clojure form values shared by tasks (light-cljs-466,
//! `crate::collections::form`), through the public embedding API and
//! JavaScript source using the private `__rt.form*` intrinsics. Programs are
//! inline synthetic snippets, not light-cljs output; the hash reference in
//! `hashes_match_the_light_cljs_runtime` copies the light-cljs runtime's
//! hash functions (`src/js_runtime.js` in the light-cljs repository).

use std::collections::HashMap;
use std::thread;

use big_boy_pants_js::frontend::compile_function;
use big_boy_pants_js::{
    CollectionError, Completion, DependencyId, DependencyKey, FormKind, JsString, KeyName,
    PathStep, Primitive, Provenance, PublishError, ResultKind, Rooted, RunOutcome, Runtime,
    ValueView, VmError, VmErrorKind,
};

fn js(s: &str) -> JsString {
    JsString::from(s)
}

fn complete(outcome: RunOutcome) -> Rooted {
    match outcome {
        RunOutcome::Complete(v) => v,
        other => panic!("expected Complete, got {other:?}"),
    }
}

/// Run the JavaScript function declaration `src` with `args` to its end.
fn call(rt: &Runtime, src: &str, args: &[&Rooted]) -> Rooted {
    let code = compile_function(src).unwrap_or_else(|e| panic!("{src}: {e}"));
    complete(rt.spawn(&code, args).unwrap().run().unwrap())
}

fn failure(rt: &Runtime, src: &str) -> VmError {
    let code = compile_function(src).unwrap_or_else(|e| panic!("{src}: {e}"));
    match rt.spawn(&code, &[]).unwrap().run().unwrap() {
        RunOutcome::Failed(e) => e,
        other => panic!("{src}: expected Failed, got {other:?}"),
    }
}

fn at(line: u32, column: u32) -> Provenance {
    Provenance {
        source: 1,
        line,
        column,
    }
}

/// Rust builds `(defthing ^{:doc "d"} [x] {[:k 1] x, {:a [1 2]} "y"})`, with
/// a vector and a map as keys, and two JavaScript tasks, running at once on
/// two threads, read it (looking keys up with separately allocated equal
/// values, a list for the vector key) and return the form, its parts and
/// the stored key objects by identity.
#[test]
fn rust_forms_are_read_and_returned_by_identity_by_two_tasks() {
    let rt = Runtime::new();
    let parts = rt.scope(|s| {
        let kw = |n: &str| s.new_keyword(None, js(n)).unwrap();
        let num = |n: f64| s.primitive(Primitive::Number(n));
        let head = s.new_symbol(None, js("defthing")).unwrap();
        let x = s.new_symbol(None, js("x")).unwrap();
        let doc = s.primitive(Primitive::String(js("d")));
        let meta = s.new_form_map(&[(kw("doc"), doc)]).unwrap();
        let params = s.new_form_vector(&[x]).unwrap();
        let params = s.form_with_meta(params, Some(meta)).unwrap();
        let x_value = s.new_symbol(None, js("x")).unwrap();
        let vec_key = s.new_form_vector(&[kw("k"), num(1.0)]).unwrap();
        let pair = s.new_form_vector(&[num(1.0), num(2.0)]).unwrap();
        let map_key = s.new_form_map(&[(kw("a"), pair)]).unwrap();
        let y = s.primitive(Primitive::String(js("y")));
        let body = s.new_form_map(&[(vec_key, x_value), (map_key, y)]).unwrap();
        let form = s.new_list(&[head, params, body]).unwrap();
        [form, params, x_value, vec_key, map_key, meta].map(|l| s.persist(l))
    });
    let form = &parts[0];
    rt.publish(form).unwrap();
    let src = "function read(form) {
        const params = __rt.formNth(form, 1), body = __rt.formNth(form, 2);
        if (__rt.formKind(form) !== 'list' || __rt.formCount(body) !== 2) throw new Error('shape');
        const x = __rt.formGet(body, __rt.formList(__rt.formKeyword(null, 'k'), 1), null);
        const y = __rt.formGet(body, __rt.formMap(__rt.formKeyword(null, 'a'), __rt.formList(1, 2)), null);
        if (y !== 'y' || __rt.formName(x) !== 'x' || __rt.formNamespace(x) !== null) throw new Error('lookup');
        const keys = __rt.formKeys(body);
        let vecKey = null, mapKey = null;
        for (let i = 0; i < __rt.formCount(keys); i++) {
            const k = __rt.formNth(keys, i);
            if (__rt.formKind(k) === 'vector') vecKey = k; else mapKey = k;
        }
        return __rt.formVector(form, params, x, vecKey, mapKey, __rt.formMeta(params));
    }";
    let code = compile_function(src).unwrap();
    let results: Vec<Rooted> = thread::scope(|sc| {
        let running: Vec<_> = (0..2)
            .map(|_| {
                let task = rt.spawn(&code, &[form]).unwrap();
                sc.spawn(move || complete(task.run().unwrap()))
            })
            .collect();
        running.into_iter().map(|t| t.join().unwrap()).collect()
    });
    for result in &results {
        rt.scope(|s| {
            let items = s.form_items(s.local(result).unwrap()).unwrap();
            assert_eq!(items.len(), parts.len());
            for (i, (got, want)) in items.iter().zip(&parts).enumerate() {
                assert!(s.same_object(*got, s.local(want).unwrap()), "item {i}");
            }
        });
    }
    // Neither task changed what Rust holds.
    rt.scope(|s| {
        let form = s.local(form).unwrap();
        assert_eq!(s.form_count(form), Ok(3));
        assert_eq!(s.form_kind(form), Some(FormKind::List));
    });
}

/// Separately allocated equal keys retrieve the same entry; keyword, symbol
/// and string spellings stay distinct; lists and vectors are sequentially
/// equal; updates keep identity rules. All from JavaScript.
#[test]
fn equal_separately_allocated_keys_retrieve_the_same_entry() {
    let rt = Runtime::new();
    let src = r#"function check() {
        const kw = (ns, n) => __rt.formKeyword(ns, n), sym = (ns, n) => __rt.formSymbol(ns, n);
        const vecKey = __rt.formVector(kw(null, 'a'), 1);
        const m = __rt.formMap(vecKey, 'v1',
            __rt.formMap(kw(null, 'x'), __rt.formList(1)), 'v2',
            sym('my.ns', 's'), 'v3',
            1, 'one',
            'str', 'v5',
            null, 'nil');
        const withLine = __rt.formWithMeta(sym('my.ns', 's'), __rt.formMap(kw(null, 'line'), 3));
        const nan = __rt.formList(NaN);
        const checks = [
            __rt.formEquals(kw(null, 'a'), kw(null, 'a')) && kw(null, 'a') !== kw(null, 'a'),
            !__rt.formEquals(kw(null, 'a'), sym(null, 'a')),
            !__rt.formEquals(kw('ns', 'a'), kw(null, 'a')),
            !__rt.formEquals(kw('', 'a'), kw(null, 'a')),
            !__rt.formEquals(kw(null, 'a'), 'a'),
            __rt.formEquals(__rt.formList(1, 2), __rt.formVector(1, 2)),
            __rt.formHash(__rt.formList(1, 2)) === __rt.formHash(__rt.formVector(1, 2)),
            !__rt.formEquals(__rt.formList(1, 2), __rt.formVector(2, 1)),
            __rt.formEquals(__rt.formMap(kw(null, 'a'), 1, kw(null, 'b'), 2), __rt.formMap(kw(null, 'b'), 2, kw(null, 'a'), 1)),
            !__rt.formEquals(__rt.formMap(kw(null, 'a'), 1), __rt.formMap(kw(null, 'a'), 2)),
            !__rt.formEquals(nan, __rt.formList(NaN)) && __rt.formEquals(nan, nan),
            __rt.formEquals(0, -0) && __rt.formEquals(null, undefined) && !__rt.formEquals(null, false),
            !__rt.formEquals(kw(null, '\ud800'), kw(null, '�')) && __rt.formName(kw(null, '\ud800')) === '\ud800',
            __rt.formGet(m, __rt.formList(kw(null, 'a'), 1), null) === 'v1',
            __rt.formGet(m, __rt.formMap(kw(null, 'x'), __rt.formVector(1)), null) === 'v2',
            __rt.formGet(m, withLine, null) === 'v3',
            __rt.formGet(m, 1.0, null) === 'one' && __rt.formGet(m, '1', 'none') === 'none',
            __rt.formGet(m, undefined, null) === 'nil',
            __rt.formGet(m, kw('my.ns', 's'), 'none') === 'none',
            __rt.formGet(m, sym(null, 's'), 'none') === 'none',
            __rt.formCount(__rt.formAssoc(m, __rt.formList(kw(null, 'a'), 1), 'w')) === 6,
            __rt.formNth(__rt.formKeys(__rt.formAssoc(__rt.formMap(vecKey, 1), __rt.formList(kw(null, 'a'), 1), 2)), 0) === vecKey,
            __rt.formAssoc(m, __rt.formList(kw(null, 'a'), 1), 'v1') === m,
            __rt.formCount(__rt.formDissoc(m, __rt.formList(kw(null, 'a'), 1))) === 5,
            __rt.formDissoc(m, kw(null, 'absent')) === m,
            __rt.formGet(__rt.formAssoc(__rt.formMap(), NaN, 1), NaN, 'none') === 'none',
            __rt.formCount(__rt.formMap(__rt.formVector(1), 'a', __rt.formList(1), 'b')) === 1,
            __rt.formGet(__rt.formMap(__rt.formVector(1), 'a', __rt.formList(1), 'b'), __rt.formVector(1), null) === 'b',
            __rt.formEquals(__rt.formConj(__rt.formList(2, 3), 1), __rt.formVector(1, 2, 3)),
            __rt.formEquals(__rt.formConj(__rt.formVector(1, 2), 3), __rt.formList(1, 2, 3)),
            __rt.formKind(__rt.formConj(__rt.formList(), 1)) === 'list',
            __rt.formGet(__rt.formVector(10, 20), 1, null) === 20 && __rt.formGet(__rt.formVector(10), 1.5, 'none') === 'none',
            __rt.formEquals(__rt.formAssoc(__rt.formVector(1, 2), 2, 3), __rt.formVector(1, 2, 3)),
            __rt.formMeta(kw(null, 'a')) === null && __rt.formKind(1) === null && __rt.formValue(1) === 1,
        ];
        return checks.indexOf(false);
    }"#;
    let result = call(&rt, src, &[]);
    assert_eq!(
        result.view(),
        ValueView::Number(-1.0),
        "first failing check"
    );
}

/// Metadata never changes equality or hashing, and does not change identity
/// rules: a new object, unless the metadata is the same object. Updates
/// keep metadata.
#[test]
fn metadata_does_not_change_equality_or_hash() {
    let rt = Runtime::new();
    rt.scope(|s| {
        let kw = |n: &str| s.new_keyword(None, js(n)).unwrap();
        let num = |n: f64| s.primitive(Primitive::Number(n));
        let meta = s.new_form_map(&[(kw("line"), num(7.0))]).unwrap();
        let other = s.new_form_map(&[(kw("line"), num(8.0))]).unwrap();
        let sym = s.new_symbol(Some(js("my.ns")), js("f")).unwrap();
        let list = s.new_list(&[sym, num(1.0)]).unwrap();
        let vec = s.new_form_vector(&[sym]).unwrap();
        let map = s.new_form_map(&[(kw("k"), vec)]).unwrap();
        for v in [sym, list, vec, map] {
            let a = s.form_with_meta(v, Some(meta)).unwrap();
            let b = s.form_with_meta(a, Some(other)).unwrap();
            assert!(!s.same_object(a, v) && !s.same_object(a, b));
            assert!(s.form_equals(a, v) && s.form_equals(a, b));
            assert_eq!(s.form_hash(a), s.form_hash(v));
            assert_eq!(s.form_hash(b), s.form_hash(v));
            // As map keys, the variants find each other's entries.
            let m = s.new_form_map(&[(a, num(1.0))]).unwrap();
            assert!(s.form_get(m, b).unwrap().is_some());
            assert!(s.form_get(m, v).unwrap().is_some());
            let same = s.form_with_meta(a, Some(meta)).unwrap();
            assert!(s.same_object(same, a), "same metadata: same object");
            let stripped = s.form_with_meta(a, None).unwrap();
            assert!(s.form_meta(stripped).is_none() && s.form_equals(stripped, a));
            assert!(s.same_object(s.form_meta(a).unwrap(), meta));
        }
        let vec = s.form_with_meta(vec, Some(meta)).unwrap();
        let updated = s.form_conj(vec, num(2.0)).unwrap();
        assert!(s.same_object(s.form_meta(updated).unwrap(), meta));
        let keyword = kw("k");
        assert_eq!(
            s.form_with_meta(keyword, Some(meta)).err(),
            Some(CollectionError::WrongType {
                expected: "symbol, list, form vector, form map, form set or record",
                found: "keyword"
            })
        );
        assert_eq!(
            s.form_with_meta(sym, Some(vec)).err(),
            Some(CollectionError::WrongType {
                expected: "form map or nil",
                found: "form vector"
            })
        );
    });
}

/// Equal scalars at different source locations are distinct occurrences:
/// equal values, different objects and provenances. A literal no Number
/// represents exactly survives an unchanged round trip through a task
/// because its occurrence (and so its provenance) is returned by identity;
/// a number the task computes has none.
#[test]
fn distinct_occurrences_and_exact_literals_survive_a_round_trip() {
    let rt = Runtime::new();
    // The host's own table: source location -> exact reader text.
    let mut exact: HashMap<(u32, u32), &str> = HashMap::new();
    let form = rt.scope(|s| {
        let mut occurrence = |text: &'static str, n: f64, line: u32| {
            exact.insert((line, 2), text);
            let value = s.primitive(Primitive::Number(n));
            s.new_occurrence(value, at(line, 2)).unwrap()
        };
        let big = occurrence("12345678901234567891", 12_345_678_901_234_567_891.0, 2);
        let one = occurrence("1", 1.0, 3);
        let one_point_zero = occurrence("1.0", 1.0, 4);
        let head = s.new_symbol(None, js("f")).unwrap();
        // Occurrences are transparent for equality, hashing and keys.
        assert!(s.form_equals(one, one_point_zero) && !s.same_object(one, one_point_zero));
        assert_ne!(s.provenance(one), s.provenance(one_point_zero));
        assert_eq!(
            s.form_hash(one),
            s.form_hash(s.primitive(Primitive::Number(1.0)))
        );
        let k = s.new_keyword(None, js("k")).unwrap();
        let k_occurrence = s.new_occurrence(k, at(9, 9)).unwrap();
        let v = s.primitive(Primitive::Bool(true));
        let m = s.new_form_map(&[(k_occurrence, v)]).unwrap();
        let k2 = s.new_keyword(None, js("k")).unwrap();
        assert!(s.form_get(m, k2).unwrap().is_some());
        assert!(s.same_object(s.form_value(k_occurrence), k));
        s.persist(s.new_list(&[head, big, one, one_point_zero]).unwrap())
    });
    rt.publish(&form).unwrap();
    let result = call(
        &rt,
        "function macro(form) {
            const big = __rt.formNth(form, 1), one = __rt.formNth(form, 2), onePointZero = __rt.formNth(form, 3);
            if (one === onePointZero || !__rt.formEquals(one, onePointZero)) throw new Error('occurrences');
            if (__rt.formKind(one) !== 'occurrence' || __rt.formValue(one) !== 1) throw new Error('value');
            return __rt.formList(__rt.formSymbol(null, 'g'), big, one, onePointZero, __rt.formValue(big) + 0);
        }",
        &[&form],
    );
    // The host emits each element: exact text by provenance, else the Number.
    let emitted: Vec<String> = rt.scope(|s| {
        let items = s.form_items(s.local(&result).unwrap()).unwrap();
        let input = s.form_items(s.local(&form).unwrap()).unwrap();
        for i in 1..4 {
            assert!(
                s.same_object(items[i], input[i]),
                "occurrence {i} by identity"
            );
        }
        items[1..]
            .iter()
            .map(|&item| match s.provenance(item) {
                Ok(p) => exact[&(p.line, p.column)].to_string(),
                Err(_) => match s.view(item) {
                    ValueView::Number(n) => format!("{n}"),
                    other => panic!("unexpected {other:?}"),
                },
            })
            .collect()
    });
    assert_eq!(
        emitted,
        ["12345678901234567891", "1", "1.0", "12345678901234567000"]
    );
}

/// A task suspended at `__rt.need` keeps a form it holds alive while the
/// host drops its own root and collects; the task resumes (on another
/// thread) and returns it by identity. Once the result and task are gone,
/// every part is reclaimed.
#[test]
fn collection_during_suspension_preserves_live_forms_and_released_roots_reclaim() {
    let rt = Runtime::new();
    let (root, weak) = rt.scope(|s| {
        let kw = s.new_keyword(Some(js("ns")), js("k")).unwrap();
        let sym = s.new_symbol(None, js("s")).unwrap();
        let key = s.new_form_vector(&[kw, sym]).unwrap();
        let value = s.new_list(&[sym]).unwrap();
        let map = s.new_form_map(&[(key, value)]).unwrap();
        let weak: Vec<_> = [map, key, kw, sym, value]
            .iter()
            .map(|l| s.downgrade(*l))
            .collect();
        (s.persist(map), weak)
    });
    let code = compile_function(
        "function hold(m) {
            const key = __rt.formVector(__rt.formKeyword('ns', 'k'), __rt.formSymbol(null, 's'));
            const before = __rt.formGet(m, key, null);
            const n = __rt.need(5, 3);
            if (__rt.formGet(m, key, null) !== before) throw new Error('changed');
            return __rt.formVector(m, before, n);
        }",
    )
    .unwrap();
    let task = rt.spawn(&code, &[&root]).unwrap();
    let key = DependencyKey::new(rt.world(), DependencyId::new(5), ResultKind::new(3));
    match task.run().unwrap() {
        RunOutcome::Waiting(k) => assert_eq!(k, key),
        other => panic!("expected Waiting, got {other:?}"),
    }
    drop(root);
    for _ in 0..2 {
        rt.request_collection().unwrap();
        assert!(
            weak.iter().all(|w| w.is_live()),
            "held by the suspended task"
        );
    }
    let mut requests = task.take_producer_requests();
    let request = requests.pop().unwrap();
    let five = rt.root(Primitive::Number(5.0));
    let mut runnable = rt
        .complete(request.key(), request.producer(), Completion::Ready(&five))
        .unwrap()
        .into_runnable();
    let resumed = runnable.pop().unwrap();
    drop(task);
    let result = thread::spawn(move || complete(resumed.run().unwrap()))
        .join()
        .unwrap();
    rt.request_collection().unwrap();
    rt.scope(|s| {
        let items = s.form_items(s.local(&result).unwrap()).unwrap();
        assert!(s.same_object(items[0], s.upgrade(&weak[0]).unwrap()));
        assert!(s.same_object(items[1], s.upgrade(&weak[4]).unwrap()));
        assert_eq!(s.view(items[2]), ValueView::Number(5.0));
    });
    drop(result);
    let stats = rt.request_collection().unwrap();
    assert!(stats.reclaimed >= weak.len());
    assert!(
        weak.iter().all(|w| !w.is_live()),
        "released forms are reclaimed"
    );
}

/// A form reaching task-owned mutation anywhere (an element, a map value,
/// a map key, metadata, or inside an occurrence) is rejected on publication
/// with the path to it. Immutable form wrappers never make it shareable.
#[test]
fn a_mutable_descendant_is_rejected_on_publication_with_a_path() {
    let rt = Runtime::new();
    let cases = [
        (
            "__rt.formList(__rt.formSymbol(null, 'do'), __rt.formMap(__rt.formKeyword(null, 'k'), __rt.formVector(1, {})))",
            "value[1]{:k}[1]",
            "object",
        ),
        (
            "__rt.formWithMeta(__rt.formSymbol('my.ns', 'f'), __rt.formMap(__rt.formSymbol('a', 'b'), [1]))",
            "value.meta{a/b}",
            "array",
        ),
        (
            "__rt.formMap(__rt.formVector({}), 1)",
            "value{<form vector>}.key[0]",
            "object",
        ),
        // A set's members, direct and nested, and its metadata
        // (light-cljs-477).
        ("__rt.formSet(1, {})", "value{<object>}.key", "object"),
        (
            "__rt.formList(__rt.formSet(__rt.formVector(2, [3])))",
            "value[0]{<form vector>}.key[1]",
            "array",
        ),
        (
            "__rt.formWithMeta(__rt.formSet(1), __rt.formMap(__rt.formKeyword(null, 'm'), {}))",
            "value.meta{:m}",
            "object",
        ),
    ];
    for (expr, path, found) in cases {
        let result = call(&rt, &format!("function make() {{ return {expr}; }}"), &[]);
        let err = rt.publish(&result).unwrap_err();
        let PublishError::NotShareable {
            path: p,
            found: f,
            owner,
        } = &err
        else {
            panic!("{expr}: {err:?}")
        };
        assert_eq!((p.to_string().as_str(), *f), (path, found), "{expr}");
        assert!(owner.is_some(), "{expr}");
        assert!(err.to_string().contains(path), "{err}");
    }
    // An occurrence (an immutable wrapper) around a task-owned array.
    let array = call(&rt, "function make() { return [1]; }", &[]);
    let wrapped = rt.scope(|s| {
        let occ = s
            .new_occurrence(s.local(&array).unwrap(), at(1, 1))
            .unwrap();
        let sym = s.new_symbol(None, js("quote")).unwrap();
        s.persist(s.new_list(&[sym, occ]).unwrap())
    });
    let err = rt.publish(&wrapped).unwrap_err();
    let PublishError::NotShareable { path, found, .. } = err else {
        panic!("{err:?}")
    };
    assert_eq!(
        path.steps(),
        [PathStep::Index(1), PathStep::OccurrenceValue]
    );
    assert_eq!(found, "array");
    // The same value cannot become a dependency result either.
    let key = DependencyKey::new(rt.world(), DependencyId::new(1), ResultKind::new(1));
    let reader = compile_function("function r() { return __rt.need(1, 1); }").unwrap();
    let task = rt.spawn(&reader, &[]).unwrap();
    assert!(matches!(task.run().unwrap(), RunOutcome::Waiting(_)));
    let request = task.take_producer_requests().pop().unwrap();
    assert_eq!(request.key(), key);
    assert!(
        rt.complete(
            request.key(),
            request.producer(),
            Completion::Ready(&wrapped)
        )
        .is_err()
    );
    // Map keys are named by spelling in paths.
    assert_eq!(KeyName::Form(":k".into()).to_string(), ":k");
}

/// Why a keyword or symbol name must split back at its first `/`, from the
/// light-cljs runtime's own source run in this engine: it compares names by
/// full spelling but hashes namespace and name, so `(keyword nil "a/b")` is
/// `=` to `(keyword "a" "b")` with another hash (the values recorded in
/// light-cljs `tests/fixtures/compiler-keyword-map-keys.tsv`), and a hash
/// map keeps both; symbols likewise (`symbol-full-spelling` in
/// `tests/fixtures/execution.tsv`). Native construction rejects the name
/// with no single split, through Rust and JavaScript alike, and on every
/// supported name (the ones the reader produces and host names such as
/// `a`/`b/c` or an empty namespace) agrees with the runtime's `=` and
/// `hash`.
#[test]
fn names_whose_spelling_does_not_split_back_are_rejected() {
    let rt = Runtime::new();
    let src = format!(
        r#"function check() {{
        const c = ({}).core;
        const a = c.keyword(null, 'a/b'), b = c.keyword('a', 'b');
        const sa = c.symbol(null, 'a/b/c'), sb = c.symbol('a/b/c');
        const runtime = [c.equal(a, b), c.hash(a), c.hash(b), c.count(c.hashMap(a, 1, b, 2)),
            c.equal(sa, sb), c.hash(sa) !== c.hash(sb), c.namespace(sb), c.name(sb)];
        // Supported names: native `=` and `hash` are the runtime's.
        const names = [[null, 'a'], [null, '/'], ['clojure.core', '/'], ['a', 'b'], ['a', 'b/c'],
            ['ns', ''], ['', 'x'], ['', '/'], [null, 'é😀'], ['\ud800', 'x\udc00']];
        const made = [];
        for (const [ns, n] of names) {{
            made.push([c.keyword(ns, n), __rt.formKeyword(ns, n)], [c.symbol(ns, n), __rt.formSymbol(ns, n)]);
        }}
        const mismatches = [];
        for (let i = 0; i < made.length; i++) {{
            const [r, f] = made[i];
            if (c.hash(r) !== __rt.formHash(f) || c.namespace(r) !== __rt.formNamespace(f) || c.name(r) !== __rt.formName(f))
                mismatches.push('hash or name ' + i);
            for (let j = 0; j < made.length; j++) {{
                if (c.equal(r, made[j][0]) !== __rt.formEquals(f, made[j][1])) mismatches.push('= ' + i + ' ' + j);
            }}
        }}
        return [...runtime, made.length, mismatches.join(', ')].join(' ');
    }}"#,
        include_str!("../../../src/js_runtime.js")
    );
    let result = call(&rt, &src, &[]);
    assert_eq!(
        result.string().map(|s| s.to_string_lossy()).as_deref(),
        Some("true 384366870 1482224565 2 true true a b/c 20 ")
    );
    // The fixture's canonical keyword hashes the same natively.
    rt.scope(|s| {
        let b = s.new_keyword(Some(js("a")), js("b")).unwrap();
        assert_eq!(s.form_hash(b), 1_482_224_565);
    });

    let ambiguous = [
        (None, "a/b"),
        (None, "a/b/c"),
        (None, "/x"),
        (Some("a/b"), "c"),
        (Some("a/"), ""),
        (Some(""), ""),
    ];
    rt.scope(|s| {
        for (ns, n) in ambiguous {
            assert_eq!(
                s.new_keyword(ns.map(js), js(n)).err(),
                Some(CollectionError::AmbiguousName),
                "keyword {ns:?} {n:?}"
            );
            assert_eq!(
                s.new_symbol(ns.map(js), js(n)).err(),
                Some(CollectionError::AmbiguousName),
                "symbol {ns:?} {n:?}"
            );
        }
    });
    for (ns, n) in ambiguous {
        let ns = ns.map_or("null".to_string(), |ns| format!("'{ns}'"));
        for op in ["formKeyword", "formSymbol"] {
            let src = format!("function f() {{ return __rt.{op}({ns}, '{n}'); }}");
            let e = failure(&rt, &src);
            assert_eq!(
                e.kind(),
                VmErrorKind::Collection(CollectionError::AmbiguousName),
                "{src}"
            );
            assert!(e.to_string().contains("split back"), "{e}");
        }
    }
}

/// The form hash is the light-cljs runtime's `hash`: checked against the
/// runtime's own functions run in this engine, and against values computed
/// by the runtime under Node.
#[test]
fn hashes_match_the_light_cljs_runtime() {
    let rt = Runtime::new();
    let src = r#"function check() {
        // Copied from the light-cljs runtime (src/js_runtime.js): rotateLeft,
        // mixK1, mixH1, fmix, hashInt, stringHash, hashChars, hashNamed and
        // mixCollectionHash. hashDouble reads the same bytes through typed
        // arrays instead of a DataView.
        function rotateLeft(value, distance) { return (value << distance) | (value >>> (32 - distance)); }
        function mixK1(value) { return Math.imul(rotateLeft(Math.imul(value, 0xcc9e2d51), 15), 0x1b873593); }
        function mixH1(hash, value) { return (Math.imul(rotateLeft(hash ^ value, 13), 5) + 0xe6546b64) | 0; }
        function fmix(hash, length) {
            hash ^= length;
            hash = Math.imul(hash ^ (hash >>> 16), 0x85ebca6b);
            hash = Math.imul(hash ^ (hash >>> 13), 0xc2b2ae35);
            return hash ^ (hash >>> 16);
        }
        function hashInt(value) { return value === 0 ? 0 : fmix(mixH1(0, mixK1(value)), 4); }
        function stringHash(value) {
            if (value == null) return 0;
            let result = 0;
            for (let i = 0; i < value.length; i++) result = Math.imul(result, 31) + value.charCodeAt(i);
            return result;
        }
        function hashChars(value) {
            let h = 0, i = 1;
            for (; i < value.length; i += 2) h = mixH1(h, mixK1(value.charCodeAt(i - 1) | (value.charCodeAt(i) << 16)));
            if (value.length & 1) h ^= mixK1(value.charCodeAt(value.length - 1));
            return fmix(h, value.length * 2);
        }
        function hashNamed(value) {
            const seed = hashChars(value.name);
            return seed ^ (stringHash(value.namespace) + 0x9e3779b9 + (seed << 6) + (seed >> 2));
        }
        function hashDouble(value) {
            const b = new Uint8Array(new Float64Array([value]).buffer);
            return ((b[0] << 24) | (b[1] << 16) | (b[2] << 8) | b[3]) ^ ((b[4] << 24) | (b[5] << 16) | (b[6] << 8) | b[7]);
        }
        function mixCollectionHash(basis, count) { return fmix(mixH1(0, mixK1(basis)), count); }
        // The runtime's `hash`, reading forms through the intrinsics.
        function hash(value) {
            value = __rt.formValue(value);
            if (value == null) return 0;
            if (typeof value === 'boolean') return value ? 1231 : 1237;
            if (typeof value === 'number') {
                if (!Number.isFinite(value)) return value === Infinity ? 2146435072 : value === -Infinity ? -1048576 : 2146959360;
                return Number.isSafeInteger(value) ? (Math.floor(value) % 2147483647) | 0 : hashDouble(value);
            }
            if (typeof value === 'string') return hashInt(stringHash(value));
            const kind = __rt.formKind(value);
            if (kind === 'keyword' || kind === 'symbol') {
                const named = {namespace: __rt.formNamespace(value), name: __rt.formName(value)};
                return (hashNamed(named) + (kind === 'keyword' ? 0x9e3779b9 : 0)) | 0;
            }
            if (kind === 'map') {
                const keys = __rt.formKeys(value), n = __rt.formCount(keys);
                let result = 0;
                for (let i = 0; i < n; i++) {
                    const k = __rt.formNth(keys, i);
                    result = (result + mixCollectionHash((Math.imul(31, 31 + hash(k)) + hash(__rt.formGet(value, k, null))) | 0, 2)) | 0;
                }
                return mixCollectionHash(result, n);
            }
            let result = 1;
            const n = __rt.formCount(value);
            for (let i = 0; i < n; i++) result = (Math.imul(result, 31) + hash(__rt.formNth(value, i))) | 0;
            return mixCollectionHash(result, n);
        }
        const kw = (ns, n) => __rt.formKeyword(ns, n), sym = (ns, n) => __rt.formSymbol(ns, n);
        const values = [null, undefined, true, false, 0, -0, 1, -1, 7, 2147483647, 2147483648, -2147483649,
            9007199254740991, -9007199254740991, 9007199254740992, 1.5, -2.75, 1e300, 5e-324, NaN, Infinity, -Infinity,
            '', 'a', 'ab', 'abc', '\ud800x', 'κλειδί', 'a long string with an odd count',
            kw(null, 'a'), kw('my.ns', 'a'), kw('', 'b'), sym(null, 'a'), sym('clojure.core', 'defn'), sym(null, 'é😀'),
            __rt.formList(), __rt.formVector(), __rt.formMap(),
            __rt.formList(1, 2), __rt.formVector(1, 2), __rt.formList(sym(null, 'f'), __rt.formVector(sym(null, 'x')), 'body'),
            __rt.formMap(kw(null, 'a'), 1), __rt.formMap(__rt.formVector(kw(null, 'k'), 1), __rt.formMap(kw(null, 'n'), null), 'x', __rt.formList(true, false)),
            __rt.formWithMeta(__rt.formVector(1, 2), __rt.formMap(kw(null, 'line'), 3))];
        let mismatches = [];
        for (let i = 0; i < values.length; i++) {
            if (__rt.formHash(values[i]) !== hash(values[i])) mismatches.push(i);
        }
        return mismatches.length === 0 ? values.length : -1 - mismatches[0];
    }"#;
    assert_eq!(call(&rt, src, &[]).view(), ValueView::Number(44.0));
    // Computed by the light-cljs runtime's functions under Node.
    rt.scope(|s| {
        let num = |n: f64| s.primitive(Primitive::Number(n));
        let kw_a = s.new_keyword(None, js("a")).unwrap();
        let expected = [
            (kw_a, -2_123_407_586),
            (
                s.new_keyword(Some(js("my.ns")), js("a")).unwrap(),
                -1_674_339_691,
            ),
            (s.new_symbol(None, js("a")).unwrap(), -482_876_059),
            (
                s.new_symbol(Some(js("clojure.core")), js("defn")).unwrap(),
                -1_504_648_747,
            ),
            (s.primitive(Primitive::String(js("abc"))), 74_834_163),
            (
                s.primitive(Primitive::String(
                    JsString::from_code_units(vec![0xD800, u16::from(b'x')]).unwrap(),
                )),
                188_715_951,
            ),
            (num(1.5), 63_551),
            (num(9_007_199_254_740_992.0), 16_451),
            (num(-7.0), -7),
            (
                s.new_form_vector(&[num(1.0), num(2.0)]).unwrap(),
                1_185_709_346,
            ),
            (s.new_list(&[num(1.0), num(2.0)]).unwrap(), 1_185_709_346),
            (s.new_list(&[]).unwrap(), -2_017_569_654),
            (s.new_form_map(&[(kw_a, num(1.0))]).unwrap(), -2_144_769_406),
        ];
        for (i, (v, h)) in expected.into_iter().enumerate() {
            assert_eq!(s.form_hash(v), h, "value {i}");
        }
    });
}

/// Sets (light-cljs-477) hash as the light-cljs runtime does, compare and
/// look members up structurally, and publish; the `__rt.clj*` core
/// operations pun `nil`, read through occurrences, and raise catchable
/// errors for unsupported kinds.
#[test]
fn sets_and_clojure_core_operations_follow_the_runtime() {
    let rt = Runtime::new();
    rt.scope(|s| {
        let num = |n: f64| s.primitive(Primitive::Number(n));
        let kw_a = s.new_keyword(None, js("a")).unwrap();
        let x = s.primitive(Primitive::String(js("x")));
        let pair = s.new_form_vector(&[num(1.0), num(2.0)]).unwrap();
        let inner = s.new_form_set(&[num(1.0)]).unwrap();
        // Computed by the light-cljs runtime's `hash` of `hash-set` under Node.
        for (members, h) in [
            (vec![], -15_128_758),
            (vec![num(1.0), num(2.0)], -1_751_407_317),
            (vec![kw_a, x, pair], 375_108_620),
            (vec![inner], -1_056_510_838),
        ] {
            let set = s.new_form_set(&members).unwrap();
            assert_eq!(s.form_hash(set), h);
            assert_eq!(s.form_kind(set), Some(FormKind::Set));
        }
        let a = s.new_form_set(&[num(1.0), pair, num(1.0)]).unwrap();
        let equal_pair = s.new_list(&[num(1.0), num(2.0)]).unwrap();
        let b = s.new_form_set(&[equal_pair, num(1.0)]).unwrap();
        assert_eq!(s.form_count(a), Ok(2));
        assert!(s.form_equals(a, b));
        assert!(s.same_object(s.form_get(a, equal_pair).unwrap().unwrap(), pair));
        let fewer = s.form_disj(a, equal_pair).unwrap();
        assert!(!s.form_equals(fewer, b));
        assert_eq!(s.form_items(fewer).unwrap().len(), 1);
        let shared = s.persist(a);
        rt.publish(&shared).unwrap();
    });
    let src = r#"function check() {
        const kw = n => __rt.formKeyword(null, n);
        const s = __rt.formSet(1, kw('a'), __rt.formVector(1));
        const checks = [
            __rt.formKind(s) === 'set',
            __rt.cljCount(s) === 3,
            __rt.cljContains(s, __rt.formList(1)),
            !__rt.cljContains(s, 2),
            __rt.cljCount(__rt.cljDisj(s, kw('a'))) === 2,
            __rt.cljDisj(s, 9) === s,
            __rt.cljConj(s, 1) === s,
            __rt.cljCount(__rt.cljSeq(s)) === 3,
            __rt.formKind(__rt.cljSeq(s)) === 'list',
            __rt.cljCount(null) === 0,
            __rt.cljFirst(null) === null,
            __rt.cljSeq(__rt.formVector()) === null,
            __rt.cljGet(null, 1, 'd') === 'd',
            __rt.cljGet(__rt.formMap(kw('k'), 1), kw('k'), null) === 1,
            __rt.cljGet(__rt.formVector(5, 6), 1, null) === 6,
            __rt.formKind(__rt.cljConj(null, 1)) === 'list',
            __rt.cljGet(__rt.cljAssoc(null, kw('k'), 2), kw('k'), null) === 2,
            __rt.cljDissoc(null, 1) === null,
            __rt.cljContains(__rt.formVector(1), 0) && !__rt.cljContains(__rt.formVector(1), 1),
            __rt.cljFirst(__rt.cljFirst(__rt.formMap(kw('k'), 1))) !== null,
            __rt.cljFirst('ab') === 'a' && __rt.cljCount('ab') === 2,
            __rt.cljNthOr(__rt.formList(1), 4, 'd') === 'd',
            __rt.cljTruth(0) && __rt.cljTruth('') && !__rt.cljTruth(null) && !__rt.cljTruth(false),
            __rt.cljCount(__rt.cljRest(__rt.formVector(1, 2, 3))) === 2,
            __rt.cljNext(__rt.formList(1)) === null,
            __rt.cljCount(__rt.cljCons(0, __rt.formVector(1))) === 2,
            __rt.cljMeta(__rt.cljWithMeta(__rt.formSet(), __rt.formMap(kw('m'), 1))) !== null,
        ];
        let message = null;
        try { __rt.cljSeq(1); } catch (e) { message = String(e); }
        let range = null;
        try { __rt.cljNth(__rt.formVector(), 0); } catch (e) { range = String(e); }
        const failed = checks.indexOf(false);
        if (failed >= 0) return 'check ' + failed;
        return message + ' | ' + range;
    }"#;
    let result = call(&rt, src, &[]);
    assert_eq!(
        result.string().map(|s| s.to_string_lossy()).as_deref(),
        Some(
            "TypeError: unsupported operation: native macro runtime seq on number | \
             RangeError: index out of bounds"
        )
    );
}

#[test]
fn scalar_predicates_and_callable_collections_preserve_occurrences() {
    let rt = Runtime::new();
    let [zero, no] = rt.scope(|s| {
        let zero = s.primitive(Primitive::Number(-0.0));
        let no = s.primitive(Primitive::Bool(false));
        [zero, no].map(|v| {
            let occurrence = s.new_occurrence(v, at(1, 1)).unwrap();
            s.persist(occurrence)
        })
    });
    let result = call(
        &rt,
        r#"function check(zero, no) {
        if (!__rt.cljIsZero(zero) || !__rt.cljIsZero(0) || __rt.cljIsZero('0') ||
            __rt.cljIsZero(null) || __rt.cljIsZero(false) || __rt.cljIsZero(NaN) ||
            !__rt.cljIsFalse(no) || __rt.cljIsFalse(0) || __rt.cljIsFalse(null)) throw new Error('predicates');
        const k = __rt.formKeyword(null, 'x'), sym = __rt.formSymbol('n', 'x');
        const m = __rt.formMap(k, no, sym, zero), v = __rt.formVector(no, zero);
        if (__rt.cljInvoke(k, m, null, 1) !== no || __rt.cljInvoke(sym, m, null, 1) !== zero ||
            __rt.cljInvoke(m, sym, null, 1) !== zero || __rt.cljInvoke(v, 0, null, 1) !== no ||
            __rt.cljInvoke(v, 0.5, null, 1) !== no || __rt.cljInvoke(v, 1.5, null, 1) !== zero ||
            __rt.cljInvoke(v, zero, null, 1) !== no ||
            __rt.cljInvoke(__rt.formSet(no), false, null, 1) !== no ||
            __rt.cljInvoke(m, 'absent', zero, 2) !== zero) throw new Error('lookup identity');
        for (const index of [-0.5, 2, NaN, Infinity, -Infinity]) {
            let message;
            try { __rt.cljInvoke(v, index, 'ignored', 2); } catch (e) { message = e.message; }
            if (message !== 'Index out of bounds') throw new Error('numeric bounds');
        }
        // Fractional truncation belongs only to the legacy callable ABI.
        if (__rt.cljGet(v, 0.5, 'missing') !== 'missing' ||
            __rt.cljNthOr(v, 0.5, 'missing') !== 'missing') throw new Error('index contract');
        let keyError, boundsError, callableError;
        try { __rt.cljInvoke(v, '0', null, 1); } catch (e) { keyError = e.message; }
        try { __rt.cljInvoke(v, 2, 'ignored', 2); } catch (e) { boundsError = e.message; }
        try { __rt.cljInvoke(null, 0, null, 1); } catch (e) { callableError = e instanceof TypeError; }
        return keyError === 'Key must be integer' && boundsError === 'Index out of bounds' && callableError;
    }"#,
        &[&zero, &no],
    );
    assert_eq!(result.view(), ValueView::Bool(true));
}

#[test]
fn map_entry_calls_require_exact_indices_and_preserve_stored_occurrences() {
    let rt = Runtime::new();
    let [zero, no] = rt.scope(|s| {
        [Primitive::Number(-0.0), Primitive::Bool(false)].map(|value| {
            let value = s.primitive(value);
            let occurrence = s.new_occurrence(value, at(1, 1)).unwrap();
            s.persist(occurrence)
        })
    });
    let result = call(
        &rt,
        r#"function check(zero, no) {
        const entry = __rt.cljFirst(__rt.formMap(zero, no));
        if (__rt.cljInvoke(entry, zero, null, 1) !== zero ||
            __rt.cljInvoke(entry, 1, null, 1) !== no ||
            __rt.cljInvoke(entry, 1, zero, 2) !== no) throw new Error('entry identity');
        for (const index of [0.5, 1.5, -0.5, -1, 2, NaN, Infinity, '0', null, undefined, false]) {
            let message;
            try { __rt.cljInvoke(entry, index, null, 1); } catch (e) { message = e.message; }
            if (message !== 'Index out of bounds') throw new Error('entry bounds');
            for (const fallback of [zero, no, null, undefined]) {
                if (__rt.cljInvoke(entry, index, fallback, 2) !== fallback) throw new Error('fallback identity');
            }
        }
        for (const count of [0, 3, 4]) {
            let message;
            try { __rt.cljInvoke(entry, 0, zero, count); } catch (e) { message = e.message; }
            if (message !== 'Invalid arity: ' + count) throw new Error('entry arity');
        }
        // Entry and vector keys still compare and hash alike.
        const vector = __rt.formVector(zero, no);
        const map = __rt.formMap(entry, no);
        if (__rt.cljGet(map, vector, null) !== no ||
            __rt.cljInvoke(vector, 0.5, null, 1) !== zero ||
            __rt.cljInvoke(__rt.cljWithMeta(entry, null), 1.5, null, 1) !== no) throw new Error('vector semantics');
        return __rt.formVector(__rt.cljInvoke(entry, zero, null, 1),
                               __rt.cljInvoke(entry, 1, null, 1),
                               __rt.cljInvoke(entry, 0.5, zero, 2),
                               __rt.cljInvoke(entry, '0', no, 2));
    }"#,
        &[&zero, &no],
    );
    rt.scope(|s| {
        let items = s.form_items(s.local(&result).unwrap()).unwrap();
        assert_eq!(items.len(), 4);
        for (got, want) in items.iter().zip([&zero, &no, &zero, &no]) {
            assert!(s.same_object(*got, s.local(want).unwrap()));
        }
    });
}

/// Form operations reject other values with a `Collection` error naming the
/// expected and found kinds; the SameValueZero collections keep their own
/// identity-keyed contract and are not forms.
#[test]
fn operand_kinds_are_checked_and_internal_maps_are_unchanged() {
    let rt = Runtime::new();
    let wrong =
        |expected, found| VmErrorKind::Collection(CollectionError::WrongType { expected, found });
    for (src, kind) in [
        (
            "__rt.formCount(1)",
            wrong("list, form vector, form map or form set", "number"),
        ),
        (
            "__rt.formCount(__rt.vector(1))",
            wrong("list, form vector, form map or form set", "vector"),
        ),
        (
            "__rt.formGet(__rt.formList(1), 0, null)",
            wrong("form vector, form map or form set", "list"),
        ),
        ("__rt.formKeyword(null, 1)", wrong("string name", "number")),
        (
            "__rt.formSymbol(1, 'a')",
            wrong("string or nil namespace", "number"),
        ),
        (
            "__rt.formName(__rt.formList())",
            wrong("keyword or symbol", "list"),
        ),
        (
            "__rt.formDissoc(__rt.formVector(), 0)",
            wrong("form map", "form vector"),
        ),
        (
            "__rt.formConj(__rt.formMap(), 1)",
            wrong("list, form vector or form set", "form map"),
        ),
        (
            "__rt.formNth(__rt.formVector(1), 3)",
            VmErrorKind::Collection(CollectionError::IndexOutOfBounds { index: 3, count: 1 }),
        ),
        (
            "__rt.formAssoc(__rt.formVector(1), -1, 0)",
            VmErrorKind::Collection(CollectionError::InvalidIndex),
        ),
    ] {
        let e = failure(&rt, &format!("function f() {{ return {src}; }}"));
        assert_eq!(e.kind(), kind, "{src}");
    }
    let src = "function f() {
        const m = __rt.map(__rt.vector(1), 'internal');
        const f = __rt.formMap(__rt.formVector(1), 'form');
        return __rt.getOr(m, __rt.vector(1), 'by identity') === 'by identity'
            && __rt.formGet(f, __rt.formVector(1), null) === 'form'
            && __rt.formKind(m) === null && __rt.formKind(__rt.vector(1)) === null
            && __rt.formEquals(__rt.vector(1), __rt.vector(1)) === false;
    }";
    assert_eq!(call(&rt, src, &[]).view(), ValueView::Bool(true));
    let err = compile_function("function f() { return __rt.formKeyword('a'); }").unwrap_err();
    assert!(err.to_string().contains("takes 2 arguments"), "{err}");
}

#[test]
fn paused_task_global_snapshot_roots_forms_without_running_accessors() {
    let rt = Runtime::new();
    let executable = big_boy_pants_js::frontend::compile_script(
        "snapshot",
        r#"
        globalThis.result = __rt.formVector(__rt.formKeyword(null, 'native'), 42);
        Object.defineProperty(globalThis, 'accessor', {get() { throw new Error('must not run'); }});
        __rt.need(91, 1);
    "#,
    )
    .unwrap();
    let task = rt.spawn(&executable, &[]).unwrap();
    {
        let _guard = task.try_begin_run().unwrap();
        assert!(matches!(
            task.global_value("result"),
            Err(big_boy_pants_js::RunError::AlreadyRunning)
        ));
    }
    assert!(matches!(task.run().unwrap(), RunOutcome::Waiting(_)));
    assert!(task.global_value("accessor").unwrap().is_none());
    assert!(task.global_value("missing").unwrap().is_none());
    let result = task.global_value("result").unwrap().unwrap();
    drop(task);
    rt.collect().unwrap();
    rt.scope(|s| {
        let result = s.local(&result).unwrap();
        assert_eq!(s.form_kind(result), Some(FormKind::Vector));
        assert_eq!(
            s.view(s.form_nth(result, 1).unwrap()),
            ValueView::Number(42.0)
        );
    });
}

#[test]
fn opaque_object_identity_is_heap_and_generation_scoped_without_rooting() {
    use std::collections::HashSet;
    let rt = Runtime::new();
    let mut identities = HashSet::new();
    let (old, weak) = rt.scope(|s| {
        assert!(
            s.object_identity(s.primitive(Primitive::Number(1.0)))
                .is_none()
        );
        let value = s.new_form_vector(&[]).unwrap();
        let identity = s.object_identity(value).unwrap();
        assert_eq!(identity, s.object_identity(value).unwrap());
        let other = s.new_form_vector(&[]).unwrap();
        assert_ne!(identity, s.object_identity(other).unwrap());
        identities.insert(identity.clone());
        (identity, s.downgrade(value))
    });
    rt.collect().unwrap();
    assert!(
        weak.upgrade().is_err(),
        "identity tokens are not collector roots"
    );
    rt.scope(|s| {
        let value = s.new_form_vector(&[]).unwrap();
        assert!(!identities.contains(&s.object_identity(value).unwrap()));
    });
    drop(weak);
    drop(rt);
    let other_runtime = Runtime::new();
    other_runtime.scope(|s| {
        let value = s.new_form_vector(&[]).unwrap();
        assert_ne!(old, s.object_identity(value).unwrap());
    });
}

/// Records, deftype instances and protocol dispatch tables
/// (light-cljs-478): struct operations from JavaScript, publication of a
/// struct with immutable fields (and rejection of one holding a JavaScript
/// object, with a path), and the GC edges of its fields.
#[test]
fn structs_follow_record_semantics_dispatch_by_type_identity_and_publish() {
    let rt = Runtime::new();
    let made = call(
        &rt,
        "function make() {
           const R = __rt.structType('my.ns/R', __rt.formVector('a', 'b'), true);
           const T = __rt.structType('my.ns/T', __rt.formVector('x'), false);
           const r = __rt.structNew(R, 1, __rt.formVector(2));
           const a = __rt.formKeyword(null, 'a'), c = __rt.formKeyword(null, 'c');
           const check = (ok, what) => { if (!ok) throw new Error(what); };
           check(__rt.formKind(r) === 'record' && __rt.formKind(__rt.structNew(T, 1)) === 'struct', 'kind');
           check(__rt.formKind(R) === 'type' && __rt.structTypeOf(r) === R, 'type');
           check(__rt.cljGet(r, a, null) === 1 && __rt.structField(r, 'a') === 1, 'field');
           const r2 = __rt.cljAssoc(r, c, 3);
           check(__rt.cljCount(r2) === 3 && __rt.cljGet(r2, c, null) === 3, 'extension');
           check(__rt.formKind(__rt.cljDissoc(r2, c)) === 'record', 'dissoc extension');
           check(__rt.formKind(__rt.cljDissoc(r, a)) === 'map', 'dissoc field');
           check(__rt.formEquals(r, __rt.cljDissoc(r2, c)), 'equal');
           check(__rt.formHash(r) === __rt.formHash(__rt.structNew(R, 1, __rt.formVector(2))), 'hash');
           check(!__rt.formEquals(__rt.structNew(T, 1), __rt.structNew(T, 1)), 'deftype identity');
           check(__rt.cljAssoc(r, a, 1) === r, 'unchanged assoc');
           const m = __rt.recordFromMap(R, __rt.formMap(a, 5, c, 6));
           check(__rt.cljGet(m, a, null) === 5 && __rt.cljGet(m, c, null) === 6, 'map->R');
           let table = __rt.map();
           table = __rt.assoc(table, R, 'R impl');
           table = __rt.assoc(table, 'string', 'string impl');
           table = __rt.assoc(table, '_', 'default impl');
           check(__rt.cljDispatch(table, r) === 'R impl' && __rt.cljDispatch(table, 'x') === 'string impl', 'dispatch');
           check(__rt.cljDispatch(table, __rt.structNew(T, 1)) === 'default impl', 'default');
           check(__rt.cljDispatch(table, {}) === undefined, 'object');
           return __rt.formVector(r, __rt.structNew(T, {}),
             __rt.cljAssoc(r, c, {}), __rt.structNew(R, 1, 2, __rt.formMap(c, {}), null),
             __rt.cljAssoc(r, c, __rt.formVector('kept')));
         }",
        &[],
    );
    let [record, holder, extended, annotated, kept] = rt.scope(|s| {
        let v = s.local(&made).unwrap();
        std::array::from_fn(|i| s.persist(s.form_nth(v, i).unwrap()))
    });
    // Extension entries and metadata are walked too.
    for (value, expected) in [(&extended, "value{:c}"), (&annotated, "value.meta{:c}")] {
        let PublishError::NotShareable { path, .. } = rt.publish(value).unwrap_err() else {
            panic!("published a record holding an object");
        };
        assert_eq!(path.to_string(), expected);
    }
    rt.publish(&record).unwrap();
    let PublishError::NotShareable { path, .. } = rt.publish(&holder).unwrap_err() else {
        panic!("published a struct holding an object");
    };
    assert_eq!(path.to_string(), "value[0]");
    drop(made);
    rt.collect().unwrap();
    // The record's fields survive collection while it is rooted.
    let field = call(
        &rt,
        "function read(r, k) { return __rt.formNth(__rt.cljGet(r, __rt.formKeyword(null, 'b'), null), 0) + __rt.formNth(__rt.cljGet(k, __rt.formKeyword(null, 'c'), null), 0); }",
        &[&record, &kept],
    );
    let read = rt.scope(|s| s.view(s.local(&field).unwrap()));
    assert!(matches!(read, ValueView::String), "{read:?}");
    rt.scope(|s| {
        assert_eq!(
            s.string(s.local(&field).unwrap())
                .unwrap()
                .to_string_lossy(),
            "2kept"
        )
    });
}

/// The `seq`, `rest` and `next` of a list or vector share its trie
/// (light-cljs-502) and publish by their own elements: a task-owned object
/// in the hidden prefix does not stop publication, and one among the
/// remaining elements is reported at its position in the tail, past the
/// first trie leaf too.
#[test]
fn shared_seq_views_publish_only_their_own_elements() {
    let rt = Runtime::new();
    let made = call(
        &rt,
        "function make() {
           let head = __rt.formVector({});
           let tail = __rt.formVector();
           for (let i = 0; i < 40; i++) {
             head = __rt.cljConj(head, i);
             tail = __rt.cljConj(tail, i === 35 ? {} : i);
           }
           for (let i = 0; i < 33; i++) tail = __rt.cljNext(tail);
           return __rt.formVector(
             __rt.cljRest(head),
             __rt.cljNext(__rt.cljSeq(head)),
             __rt.cljRest(__rt.formList({}, 1)),
             tail);
         }",
        &[],
    );
    let [rest, next, list_rest, tail] = rt.scope(|s| {
        let v = s.local(&made).unwrap();
        std::array::from_fn(|i| s.persist(s.form_nth(v, i).unwrap()))
    });
    for view in [&rest, &next, &list_rest] {
        rt.publish(view).unwrap();
    }
    rt.scope(|s| {
        let tail = s.local(&tail).unwrap();
        assert_eq!(s.form_kind(tail), Some(FormKind::List));
        assert_eq!(s.form_count(tail), Ok(7));
    });
    let PublishError::NotShareable { path, found, .. } = rt.publish(&tail).unwrap_err() else {
        panic!("published a tail holding an object");
    };
    assert_eq!((path.to_string().as_str(), found), ("value[2]", "object"));
}
