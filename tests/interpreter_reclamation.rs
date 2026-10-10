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
fn retained_native_transforms_and_cyclic_partial_reducers_reclaim_all_captures() {
    let mut c = warmed();
    let mut baseline = None;
    for _ in 0..64 {
        let source = c.source("native-xf.clj", "(let [cell (volatile! nil) kept [10] xf (comp (map (fn [x] (if @cell (+ (first kept) x) x))) (filter (fn [x] (>= x (first kept)))) (remove (fn [x] (< x (first kept))))) r (xf (completing conj))] (vreset! cell [xf r (partial r [])]) [xf r])");
        let form = c.reader(&source).unwrap().read_form().unwrap().unwrap();
        let pair = c.eval_form(&form).unwrap();
        drop((source, form));
        let xf = pair.nth(0).unwrap().unwrap();
        let rf = pair.nth(1).unwrap().unwrap();
        drop(pair);
        c.collect_garbage();
        assert_eq!(c.memory_stats().executable_units, 1);
        let result = c
            .invoke(&rf, &[c.vector(&[]).unwrap(), c.integer(2)])
            .unwrap();
        assert_eq!(integer(result.nth(0).unwrap().unwrap()), 12);
        let into = c.eval_string("into").unwrap();
        let result2 = c
            .invoke(&into, &[c.vector(&[]).unwrap(), xf.clone(), result.clone()])
            .unwrap();
        assert_eq!(integer(result2.nth(0).unwrap().unwrap()), 22);
        drop((result, result2, rf, xf, into));
        let after = live(c.collect_garbage().after);
        assert_eq!((after.executable_units, after.sources), (0, 0));
        if let Some(baseline) = baseline {
            assert_eq!(after, baseline);
        } else {
            baseline = Some(after);
        }
    }
}

#[test]
fn into_repeated_processes_release_cyclic_transforms_and_targets() {
    let mut c = warmed();
    let mut baseline = None;
    for _ in 0..64 {
        let value = c.eval_string("(let [xf (fn [rf] (let [cell (volatile! nil) step (fn ([a] (if @cell (rf a) a)) ([a x] (rf a x)))] (vreset! cell step) step))] (into (with-meta [] {:kept [1 2]}) xf [1 2 3]))").unwrap();
        assert_eq!(value.count(), Some(3));
        drop(value);
        let after = live(c.collect_garbage().after);
        assert_eq!((after.executable_units, after.sources), (0, 0));
        if let Some(baseline) = baseline {
            assert_eq!(after, baseline);
        } else {
            baseline = Some(after);
        }
    }
}

#[test]
fn retained_transducers_keep_source_code_and_reclaim_cyclic_process_state() {
    let mut c = warmed();
    let source = c.source("saved-xf.clj", "(let [kept 10] (fn [rf] (let [n (volatile! 0)] (fn ([] (rf)) ([a] (rf a)) ([a x] (vswap! n inc) (rf a (+ kept @n x)))))))");
    let form = c.reader(&source).unwrap().read_form().unwrap().unwrap();
    let xf = c.eval_form(&form).unwrap();
    drop((form, source));
    c.collect_garbage();
    assert_eq!(c.memory_stats().executable_units, 1);
    let transduce = c.eval_string("transduce").unwrap();
    let rf = c.eval_string("(completing +)").unwrap();
    for _ in 0..3 {
        let values = c.vector(&[c.integer(1), c.integer(2)]).unwrap();
        assert_eq!(
            integer(
                c.invoke(&transduce, &[xf.clone(), rf.clone(), c.integer(0), values])
                    .unwrap()
            ),
            26
        );
        c.collect_garbage();
    }
    drop((transduce, rf, xf));
    let after = c.collect_garbage().after;
    assert_eq!((after.executable_units, after.sources), (0, 0));
    let mut baseline = None;
    for _ in 0..64 {
        let value = c.eval_string("(let [cell (volatile! nil) rf (completing (fn ([] []) ([a x] (conj a x))) (fn [a] (if @cell a a)))] (vreset! cell rf) (transduce identity rf [] [1 2 3]))").unwrap();
        assert_eq!(value.count(), Some(3));
        drop(value);
        let after = live(c.collect_garbage().after);
        assert_eq!((after.executable_units, after.sources), (0, 0));
        if let Some(baseline) = baseline {
            assert_eq!(after, baseline);
        } else {
            baseline = Some(after);
        }
    }
}

#[test]
fn counted_reducers_reclaim_failed_abandoned_and_cyclic_state() {
    let mut c = warmed();
    let mut baseline = None;
    for _ in 0..32 {
        for transform in [
            "(take 2)",
            "(drop 1)",
            "(take-while (fn [x] (if @cell (pos? x) false)))",
            "(take [cell])",
            "(drop [cell])",
        ] {
            let text = format!(
                "(let [cell (volatile! nil) xf {transform} r (xf (fn ([a] a) ([a x] (if @cell (conj a x) a))))] (vreset! cell [xf r]) [xf r])"
            );
            let source = c.source("counted-reducer.clj", text);
            let form = c.reader(&source).unwrap().read_form().unwrap().unwrap();
            let pair = c.eval_form(&form).unwrap();
            let reducer = pair.nth(1).unwrap().unwrap();
            drop((source, form, pair));
            c.collect_garbage();
            let result = c.invoke(&reducer, &[c.vector(&[]).unwrap(), c.integer(1)]);
            assert_eq!(result.is_err(), transform.contains("[cell]"));
            // Drop before exhaustion/completion, including after failed state
            // arithmetic. No host root may keep the private state cycle alive.
            drop((result, reducer));
            let after = live(c.collect_garbage().after);
            assert_eq!((after.executable_units, after.sources), (0, 0));
            if let Some(baseline) = baseline {
                assert_eq!(after, baseline);
            } else {
                baseline = Some(after);
            }
        }
    }
}

#[test]
fn cat_reclaims_failed_abandoned_reducers_and_cyclic_captures() {
    let mut c = warmed();
    let mut baseline = None;
    for _ in 0..32 {
        for fail in [false, true] {
            let text = format!(
                "(let [cell (volatile! nil) kept [10] xf (comp (mapcat (fn [x] (if @cell [x] []))) cat (take 3)) r (xf (fn ([a] a) ([a x] (if (= x 2) {} (conj a [kept x])))))] (vreset! cell [xf r (lazy-seq [cell])]) r)",
                if fail {
                    "(throw (ex-info \"stopped\" {:cell cell}))"
                } else {
                    "(conj a [kept x])"
                }
            );
            let source = c.source("cat-reducer.clj", text);
            let form = c.reader(&source).unwrap().read_form().unwrap().unwrap();
            let reducer = c.eval_form(&form).unwrap();
            drop((source, form));
            c.collect_garbage();
            assert_eq!(c.memory_stats().executable_units, 1);
            let inputs = c.vector(&[c.integer(1), c.integer(2)]).unwrap();
            let result = c.invoke(&reducer, &[c.vector(&[]).unwrap(), inputs]);
            assert_eq!(result.is_err(), fail);
            drop((result, reducer));
            let after = live(c.collect_garbage().after);
            assert_eq!((after.executable_units, after.sources), (0, 0));
            if let Some(baseline) = baseline {
                assert_eq!(after, baseline);
            } else {
                baseline = Some(after);
            }
        }
    }
}

#[test]
fn partition_buffers_reclaim_failed_abandoned_and_cyclic_inputs() {
    let mut c = warmed();
    let mut baseline = None;
    for _ in 0..32 {
        for outcome in ["success", "failure", "abandon"] {
            let step = if outcome == "failure" {
                "(throw (ex-info \"partition\" {}))"
            } else {
                "(conj a x)"
            };
            let source = c.source("partition-buffer.clj", format!(
                "(let [cell (volatile! nil) r ((partition-all 3) (fn ([a] (if @cell a a)) ([a x] {step})))] (vreset! cell r) (r [] [cell r (fn [] 42)]) r)"
            ));
            let form = c.reader(&source).unwrap().read_form().unwrap().unwrap();
            let reducer = c.eval_form(&form).unwrap();
            drop((source, form));
            c.collect_garbage();
            assert_eq!(c.memory_stats().executable_units, 1);
            if outcome != "abandon" {
                let result = c.invoke(&reducer, &[c.vector(&[]).unwrap()]);
                if outcome == "success" {
                    let result = result.unwrap();
                    let group = result.nth(0).unwrap().unwrap();
                    let item = group.nth(0).unwrap().unwrap();
                    assert_eq!(
                        integer(c.invoke(&item.nth(2).unwrap().unwrap(), &[]).unwrap()),
                        42
                    );
                } else {
                    assert!(result.is_err());
                    // A failed flush cleared the buffer before the callback.
                    assert_eq!(
                        c.invoke(&reducer, &[c.vector(&[]).unwrap()])
                            .unwrap()
                            .count(),
                        Some(0)
                    );
                }
            }
            drop(reducer);
            let after = live(c.collect_garbage().after);
            assert_eq!((after.executable_units, after.sources), (0, 0));
            if let Some(baseline) = baseline {
                assert_eq!(after, baseline);
            } else {
                baseline = Some(after);
            }
        }
    }
}

#[test]
fn counted_reducer_state_survives_fork_and_reset_without_changing_owner() {
    let mut c = warmed();
    let reducer = c.eval_string("((take 2) (completing conj))").unwrap();
    let first = c
        .invoke(&reducer, &[c.vector(&[]).unwrap(), c.integer(1)])
        .unwrap();
    let mut child = c.fork().unwrap();
    c.reset();
    assert!(
        c.invoke(&reducer, &[])
            .unwrap_err()
            .to_string()
            .contains("WrongOwner")
    );
    drop(c);
    child.collect_garbage();
    let second = child.invoke(&reducer, &[first, child.integer(2)]).unwrap();
    let value = second.reduced_value().unwrap();
    assert_eq!(value.count(), Some(2));
    assert_eq!(integer(value.nth(1).unwrap().unwrap()), 2);
    drop((value, second, reducer));
    let after = child.collect_garbage().after;
    assert_eq!((after.sources, after.executable_units), (0, 0));
}

#[test]
fn ensure_reduced_retains_and_reclaims_captured_code_on_both_paths() {
    for already_reduced in [false, true] {
        let mut c = warmed();
        let ensure = c.eval_string("ensure-reduced").unwrap();
        let closure = c.eval_string("(let [x [\"kept\"]] (fn [] x))").unwrap();
        let closure_id = closure.heap_id().unwrap();
        let argument = if already_reduced {
            c.reduced(&closure).unwrap()
        } else {
            closure.clone()
        };
        let marker = c.invoke(&ensure, std::slice::from_ref(&argument)).unwrap();
        assert_eq!(marker.same_identity(&argument), already_reduced);
        drop((ensure, argument, closure));
        c.collect_garbage();
        assert_eq!(c.memory_stats().executable_units, 1);
        let mut child = c.fork().unwrap();
        c.reset();
        assert_eq!(
            c.heap_value(closure_id).unwrap_err(),
            HandleError::WrongOwner
        );
        drop(c);
        child.collect_garbage();
        let retained = marker.reduced_value().unwrap();
        assert_eq!(
            child
                .invoke(&retained, &[])
                .unwrap()
                .nth(0)
                .unwrap()
                .unwrap()
                .inspect(),
            ValueView::String("kept".encode_utf16().collect())
        );
        drop((marker, retained));
        let after = child.collect_garbage().after;
        assert_eq!((after.sources, after.executable_units), (0, 0));
        assert_eq!(
            child.heap_value(closure_id).unwrap_err(),
            HandleError::StaleId
        );
    }
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

#[test]
fn protocol_descriptors_collect_after_unmapping_and_rejected_declarations_do_not_leak() {
    let mut c = warmed();
    let mut baseline = None;
    for _ in 0..48 {
        c.eval_string("(defprotocol P (f [x]))").unwrap();
        let p = c.eval_string("P").unwrap();
        let f = c.eval_string("f").unwrap();
        assert!(c.eval_string("(defprotocol P (f [x] [x y]))").is_err());
        assert!(c.eval_string("(defprotocol Bad (g [x & xs]))").is_err());
        let ns = c.current_namespace().namespace_id().unwrap();
        c.unmap_var(ns, &Name::new(None, "P")).unwrap();
        c.unmap_var(ns, &Name::new(None, "f")).unwrap();
        c.collect_garbage();
        assert_eq!(
            p.inspect(),
            ValueView::Protocol(Name::new(Some("user"), "P"))
        );
        assert!(c.invoke(&f, &[c.nil()]).is_err());
        drop((p, f));
        let after = live(c.collect_garbage().after);
        assert_eq!((after.executable_units, after.sources), (0, 0));
        if let Some(expected) = baseline {
            assert_eq!(after, expected);
        } else {
            baseline = Some(after);
        }
    }
}

#[test]
fn reify_receiver_capture_cycles_and_returned_closures_collect_without_slot_growth() {
    let mut c = warmed();
    c.eval_string("(defprotocol P (p [x]))").unwrap();
    let method = c.eval_string("p").unwrap();
    let mut baseline = None;
    for _ in 0..64 {
        let source = c.source("reify-capture.clj", "(let [cell (volatile! nil) kept [42] o (reify P (p [this] (fn [] (if (= @cell this) (first kept) 0))))] (vreset! cell o) o)");
        let form = c.reader(&source).unwrap().read_form().unwrap().unwrap();
        let object = c.eval_form(&form).unwrap();
        drop((source, form));
        c.collect_garbage();
        let closure = c.invoke(&method, std::slice::from_ref(&object)).unwrap();
        let id = object.heap_id().unwrap();
        drop(object);
        c.collect_garbage();
        assert_eq!(c.heap_value(id).unwrap().inspect(), ValueView::Reify);
        assert_eq!(integer(c.invoke(&closure, &[]).unwrap()), 42);
        drop(closure);
        let after = c.collect_garbage().after;
        assert!(matches!(c.heap_value(id), Err(HandleError::StaleId)));
        assert_eq!((after.executable_units, after.sources), (0, 0));
        if let Some(expected) = baseline {
            assert_eq!(after, expected);
        } else {
            baseline = Some(after);
        }
    }
}

#[test]
fn record_generations_fields_metadata_and_cycles_reclaim_to_warmed_counts() {
    let mut c = warmed();
    let mut baseline = None;
    for _ in 0..64 {
        c.eval_string("(defrecord R [cell value])").unwrap();
        let old_type = c.eval_string("R").unwrap();
        let type_id = old_type.heap_id().unwrap();
        let object = c.eval_string("(let [cell (atom nil) kept [1] r (with-meta (->R cell (map (fn [n] (+ n (first kept))) [1 2])) {:kept [7]})] (reset! cell r) r)").unwrap();
        let object_id = object.heap_id().unwrap();
        c.eval_string("(defrecord R [x])").unwrap();
        drop(old_type);
        c.collect_garbage();
        assert_eq!(
            c.heap_value(type_id).unwrap().inspect(),
            ValueView::RecordType(Name::new(Some("user"), "R"))
        );
        assert_eq!(
            object
                .metadata()
                .unwrap()
                .get(&c.keyword(Name::new(None, "kept")))
                .unwrap()
                .unwrap()
                .count(),
            Some(1)
        );
        let values = object
            .get(&c.keyword(Name::new(None, "value")))
            .unwrap()
            .unwrap();
        assert_eq!(
            c.sequence_elements(&values)
                .unwrap()
                .into_iter()
                .map(integer)
                .collect::<Vec<_>>(),
            [2, 3]
        );
        drop(values);
        drop(object);
        let after = live(c.collect_garbage().after);
        assert!(matches!(c.heap_value(type_id), Err(HandleError::StaleId)));
        assert!(matches!(c.heap_value(object_id), Err(HandleError::StaleId)));
        if let Some(expected) = baseline {
            assert_eq!(after, expected);
        } else {
            baseline = Some(after);
        }
    }
    let constructor = c.eval_string("->R").unwrap();
    let namespace = c.current_namespace().namespace_id().unwrap();
    for name in ["R", "->R", "map->R"] {
        c.unmap_var(namespace, &Name::new(None, name)).unwrap();
    }
    c.collect_garbage();
    let record = c.invoke(&constructor, &[c.integer(42)]).unwrap();
    assert_eq!(
        record
            .get(&c.keyword(Name::new(None, "x")))
            .unwrap()
            .unwrap()
            .inspect(),
        ValueView::Integer(42)
    );
    drop((record, constructor));
    let after = c.collect_garbage().after;
    assert_eq!((after.sources, after.executable_units), (0, 0));
}

#[test]
fn record_method_generations_release_captures_and_keep_returned_closure_cycles() {
    let mut c = warmed();
    c.eval_string("(defprotocol P (p [x]))").unwrap();
    let method = c.eval_string("p").unwrap();
    let mut baseline = None;
    for _ in 0..64 {
        c.eval_string("(let [kept [40]] (defrecord R [cell x] P (p [this] (fn [] (if (= @cell this) (+ x (first kept) (:m (meta this))) 0)))))").unwrap();
        let old_type = c.eval_string("R").unwrap().heap_id().unwrap();
        let object = c
            .eval_string("(let [a (atom nil) r (with-meta (->R a 1) {:m 1})] (reset! a r) r)")
            .unwrap();
        let closure = c.invoke(&method, std::slice::from_ref(&object)).unwrap();
        let object_id = object.heap_id().unwrap();
        c.eval_string("(defrecord R [x] P (p [_] x))").unwrap();
        drop(object);
        c.collect_garbage();
        assert!(c.heap_value(old_type).is_ok());
        assert!(c.heap_value(object_id).is_ok());
        assert_eq!(integer(c.invoke(&closure, &[]).unwrap()), 42);
        drop(closure);
        let after = live(c.collect_garbage().after);
        assert!(matches!(c.heap_value(old_type), Err(HandleError::StaleId)));
        assert!(matches!(c.heap_value(object_id), Err(HandleError::StaleId)));
        if let Some(expected) = baseline {
            assert_eq!(after, expected);
        } else {
            baseline = Some(after);
        }
    }
    let namespace = c.current_namespace().namespace_id().unwrap();
    for name in ["R", "->R", "map->R"] {
        c.unmap_var(namespace, &Name::new(None, name)).unwrap();
    }
    let after = c.collect_garbage().after;
    assert_eq!((after.sources, after.executable_units), (0, 0));
}
#[test]
fn replaced_external_methods_and_obsolete_generations_collect_to_warmed_counts() {
    let mut c = warmed();
    c.eval_string("(defprotocol P (p [x])) (defrecord R [n]) (extend-type R P (p [r] (:n r)))")
        .unwrap();
    let mut baseline = None;
    for _ in 0..64 {
        let capture = c
            .eval_string(
                "(let [kept [40]] (extend-type R P (p [r] (fn [] (+ (:n r) (first kept))))) kept)",
            )
            .unwrap();
        let capture_id = capture.heap_id().unwrap();
        let closure = c.eval_string("(p (->R 2))").unwrap();
        drop(capture);
        c.eval_string("(extend-type R P (p [r] (:n r)))").unwrap();
        c.collect_garbage();
        assert!(c.heap_value(capture_id).is_ok());
        assert_eq!(integer(c.invoke(&closure, &[]).unwrap()), 42);
        drop(closure);
        assert!(c.eval_string("(extend-type R P (p [_] invalid))").is_err());
        let old_type = c.eval_string("R").unwrap().heap_id().unwrap();
        c.eval_string("(defrecord R [n]) (extend-type R P (p [r] (:n r)))")
            .unwrap();
        let after = c.collect_garbage().after;
        assert!(matches!(c.heap_value(old_type), Err(HandleError::StaleId)));
        assert!(matches!(
            c.heap_value(capture_id),
            Err(HandleError::StaleId)
        ));
        if let Some(expected) = baseline {
            assert_eq!(after, expected);
        } else {
            baseline = Some(after);
        }
    }
}
