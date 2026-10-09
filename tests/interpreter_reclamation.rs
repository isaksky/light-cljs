//! Bounded long-lived embedding workloads. These check ownership, not JVM GC timing.
use light_cljs::interpreter::{
    Arity, Context, HandleError, LoadRequest, MemoryStats, Name, NativeDefinition, NativeEffects,
    NativeMetadata, RootedValue, SourceInput, ValueView,
};
use std::{cell::Cell, rc::Rc};

fn integer(value: RootedValue) -> i64 {
    let ValueView::Integer(value) = value.inspect() else {
        panic!("integer result expected")
    };
    value
}

fn live(mut stats: MemoryStats) -> MemoryStats {
    // Vacant arena capacity is deliberately retained for the next evaluation.
    stats.slots = 0;
    stats
}

fn warmed() -> Context {
    let mut c = Context::new();
    c.eval_string("(ns-map 'clojure.core)").unwrap();
    c.collect_garbage();
    c
}

#[test]
fn collection_tree_tracing_keeps_wide_vectors_map_keys_collisions_and_sets_live() {
    let mut c = Context::new();
    let values: Vec<_> = (0..1_025)
        .map(|n| c.string(format!("value-{n}").encode_utf16().collect::<Vec<_>>()))
        .collect();
    let vector = c.vector(&values).unwrap();
    let pairs: Vec<_> = values.iter().map(|v| (v.clone(), vector.clone())).collect();
    let mut map = c.map(&pairs).unwrap();
    let a = c.string("Aa".encode_utf16().collect::<Vec<_>>());
    let b = c.string("BB".encode_utf16().collect::<Vec<_>>());
    assert_eq!(a.value_hash(), b.value_hash());
    map = c.assoc(&map, &a, &vector).unwrap();
    map = c.assoc(&map, &b, &vector).unwrap();
    let set = c.set(&[a.clone(), b.clone()]).unwrap();
    let ids: Vec<_> = values.iter().map(|v| v.heap_id().unwrap()).collect();
    let (a_id, b_id) = (a.heap_id().unwrap(), b.heap_id().unwrap());
    drop((values, pairs, vector, a, b));
    c.collect_garbage();
    assert_eq!(map.count(), Some(1_027));
    for (n, id) in ids.iter().enumerate() {
        let key = c.heap_value(*id).unwrap();
        let value = map.get(&key).unwrap().unwrap();
        assert!(value.nth(n).unwrap().unwrap().same_identity(&key));
    }
    drop(map);
    c.collect_garbage();
    assert!(
        ids.iter()
            .all(|id| matches!(c.heap_value(*id), Err(HandleError::StaleId)))
    );
    for id in [a_id, b_id] {
        assert!(set.contains(&c.heap_value(id).unwrap()).unwrap());
    }
    drop(set);
    assert_eq!(c.collect_garbage().after.heap_values, 0);
}

static UNMAP: NativeDefinition = NativeDefinition {
    metadata: NativeMetadata {
        namespace: "host",
        name: "unmap",
        arity: Arity::Exact(3),
        effects: NativeEffects::Context,
        errors: &[],
    },
    callback: |c, args| {
        let ValueView::Symbol(name) = args[2].inspect() else {
            panic!("raw macro symbol")
        };
        c.unmap_var(c.current_namespace().namespace_id().unwrap(), &name)
            .unwrap();
        c.collect_garbage();
        Ok(c.integer(99))
    },
};

#[test]
fn pending_analyzer_vars_are_roots_during_effectful_macroexpansion() {
    for expression in [
        "[x (host/unmap x)]",
        "(def x (host/unmap x))",
        "(set! x (host/unmap x))",
    ] {
        let mut c = warmed();
        c.register_macro(&UNMAP);
        c.eval_string("(def ^:dynamic x 42)").unwrap();
        let source = c.source("macro-gc.clj", expression);
        let form = c.reader(&source).unwrap().read_form().unwrap().unwrap();
        let analyzed = c.analyze_form(&form).unwrap();
        drop((source, form));
        assert!(c.resolve_var(&Name::new(None, "x")).unwrap().is_none());
        c.collect_garbage();
        match expression {
            "[x (host/unmap x)]" => {
                let result = c.eval_analyzed(&analyzed).unwrap();
                assert_eq!(integer(result.nth(0).unwrap().unwrap()), 42);
                assert_eq!(integer(result.nth(1).unwrap().unwrap()), 99);
            }
            "(def x (host/unmap x))" => {
                let var = c.eval_analyzed(&analyzed).unwrap();
                assert_eq!(integer(c.var_root(var.var_id().unwrap()).unwrap()), 99);
            }
            _ => {
                // Assignment still requires a dynamic binding; collection must
                // not turn that ordinary language error into a stale-ID panic.
                assert!(c.eval_analyzed(&analyzed).is_err());
            }
        }
        drop(analyzed);
        let after = c.collect_garbage().after;
        assert_eq!((after.sources, after.executable_units), (0, 0));
    }
}

#[test]
fn redefining_cyclic_closures_reclaims_old_code_sources_and_generations() {
    let mut c = warmed();
    let baseline = live(c.memory_stats());
    let mut retained = None;
    let mut steady = None;
    for n in 0..128 {
        let source = c.source(
            format!("redefinition-{n}.clj"),
            format!("(def current (let [a (atom nil) n {n}] (letfn [(f [] (if @a n (f)))] (reset! a f) f))) current"),
        );
        let result = c.eval_source(&source).unwrap();
        let old_id = result.heap_id().unwrap();
        let source_id = source.id();
        assert_eq!(integer(c.invoke(&result, &[]).unwrap()), n);
        if n == 0 {
            retained = Some(result.clone());
        }
        drop((source, result));
        c.collect_garbage();
        assert_eq!(
            integer(c.invoke(retained.as_ref().unwrap(), &[]).unwrap()),
            0
        );
        if n > 1 {
            assert_eq!(live(c.collect_garbage().after), steady.unwrap());
        } else if n == 1 {
            steady = Some(live(c.collect_garbage().after));
        }
        // A superseded root becomes stale on the following collection. Both
        // the closure cycle and its source must die together.
        if n > 0 {
            c.eval_string("(def current nil)").unwrap();
            c.collect_garbage();
            assert_eq!(c.heap_value(old_id).unwrap_err(), HandleError::StaleId);
            assert_eq!(c.source_by_id(source_id).unwrap_err(), HandleError::StaleId);
        }
    }
    drop(retained);
    c.eval_string("(ns-unmap 'user 'current)").unwrap();
    assert_eq!(live(c.collect_garbage().after), baseline);
}

#[test]
fn discarded_lazy_prefixes_and_cached_failure_cycles_return_to_the_same_baseline() {
    let mut c = warmed();
    let baseline = live(c.memory_stats());
    let program = "(let [a (atom nil)]
      (letfn [(step [n] (lazy-seq (cons [n a] (step (inc n)))))]
        (reset! a (delay (throw (ex-info \"cycle\" {:self a}))))
        (try (force @a) (catch Exception e nil))
        (loop [xs (step 0) n 0]
          (if (= n 128) (first (first xs)) (recur (next xs) (inc n))))))";
    let mut slots = None;
    for _ in 0..48 {
        assert_eq!(integer(c.eval_string(program).unwrap()), 128);
        let after = c.collect_garbage().after;
        assert_eq!(live(after), baseline);
        if let Some(slots) = slots {
            assert_eq!(after.slots, slots, "arena vacancies should be reused");
        } else {
            slots = Some(after.slots);
        }
    }
}

#[test]
fn dropped_forks_release_private_namespaces_values_code_and_sources() {
    let mut parent = warmed();
    let baseline = live(parent.memory_stats());
    let mut slots = None;
    for n in 0..64 {
        let mut child = parent.fork().unwrap();
        let function = child
            .eval_string(&format!(
                "(ns branch-{n}) (def cell (atom nil)) (reset! cell (fn [] cell)) (fn [] @cell)"
            ))
            .unwrap();
        parent.collect_garbage();
        let nested = child.invoke(&function, &[]).unwrap();
        assert!(matches!(nested.inspect(), ValueView::Function));
        drop((nested, function, child));
        let after = parent.collect_garbage().after;
        assert_eq!(live(after), baseline, "fork {n}");
        if let Some(slots) = slots {
            assert_eq!(after.slots, slots);
        } else {
            slots = Some(after.slots);
        }
    }
}

#[test]
fn module_reload_cache_and_retained_handles_survive_reset_and_reclaim_history() {
    let mut parent = warmed();
    let revision = Rc::new(Cell::new(0));
    let calls = Rc::new(Cell::new(0));
    let (version, loaded) = (revision.clone(), calls.clone());
    parent.set_source_provider(move |_: &LoadRequest| {
        loaded.set(loaded.get() + 1);
        Ok(Some(SourceInput::new(
            format!("cached-{}.clj", version.get()),
            format!(
                "(ns cached) (def read-version (let [n {}] (fn [] n)))",
                version.get()
            ),
        )))
    });
    parent.eval_string("(require 'cached)").unwrap();
    let retained = parent.eval_string("cached/read-version").unwrap();
    let mut child = parent.fork().unwrap();
    child.eval_string("(require 'cached)").unwrap();
    assert_eq!(calls.get(), 1, "fork copies successful module records");
    // Keep the old closure callable through the same owner after reset/drop.
    parent.reset();
    assert_eq!(
        parent.heap_value(retained.heap_id().unwrap()).unwrap_err(),
        HandleError::WrongOwner
    );
    drop(parent);
    assert_eq!(integer(child.invoke(&retained, &[]).unwrap()), 0);
    let mut steady = None;
    for n in 1..65 {
        revision.set(n);
        child.eval_string("(require 'cached :reload)").unwrap();
        assert_eq!(
            integer(child.eval_string("(cached/read-version)").unwrap()),
            n
        );
        assert_eq!(integer(child.invoke(&retained, &[]).unwrap()), 0);
        let after = live(child.collect_garbage().after);
        if let Some(steady) = steady {
            assert_eq!(after, steady);
        } else {
            steady = Some(after);
        }
    }
    assert_eq!(calls.get(), 65);
    drop(retained);
    child
        .eval_string("(ns-unmap 'cached 'read-version)")
        .unwrap();
    let after = child.collect_garbage().after;
    assert_eq!((after.sources, after.executable_units), (0, 0));
    assert!(
        child
            .resolve_var(&Name::new(Some("cached"), "read-version"))
            .unwrap()
            .is_none()
    );
}
