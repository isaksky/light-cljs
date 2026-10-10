//! Eager transducing processes and ordinary callable reducer adapters.
use light_cljs::interpreter::*;
use std::cell::RefCell;
#[path = "support/sci_native.rs"]
mod native;
#[path = "support/sci_protocol.rs"]
#[allow(dead_code)]
mod protocol;

#[test]
fn custom_transducer_programs_match_pinned_sci() {
    let cases = protocol::cases(include_str!("fixtures/sci-outcomes.tsv")).unwrap();
    let mut count = 0;
    for case in cases.iter().filter(|c| c.id.starts_with("sci-transduce-")) {
        assert_eq!(
            native::run(case.source, case.profile).unwrap_or_else(|e| panic!("{}: {e}", case.id)),
            native::Observation::expected(case),
            "{}: {}",
            case.id,
            case.source
        );
        count += 1;
    }
    assert_eq!(count, 40);
}

#[test]
fn persistent_transducing_into_matches_pinned_sci() {
    let cases = protocol::cases(include_str!("fixtures/sci-outcomes.tsv")).unwrap();
    let mut count = 0;
    for case in cases.iter().filter(|c| c.id.starts_with("sci-into-xf-")) {
        assert_eq!(
            native::run(case.source, case.profile).unwrap_or_else(|e| panic!("{}: {e}", case.id)),
            native::Observation::expected(case),
            "{}: {}",
            case.id,
            case.source
        );
        count += 1;
    }
    assert_eq!(count, 32);
}

#[test]
fn native_map_filter_remove_adapters_match_pinned_sci() {
    let cases = protocol::cases(include_str!("fixtures/sci-outcomes.tsv")).unwrap();
    let mut count = 0;
    for case in cases.iter().filter(|c| c.id.starts_with("sci-xf-basic-")) {
        assert_eq!(
            native::run(case.source, case.profile).unwrap_or_else(|e| panic!("{}: {e}", case.id)),
            native::Observation::expected(case),
            "{}: {}",
            case.id,
            case.source
        );
        count += 1;
    }
    assert_eq!(count, 47);
}

#[test]
fn native_bounded_adapters_match_pinned_sci() {
    let cases = protocol::cases(include_str!("fixtures/sci-outcomes.tsv")).unwrap();
    let cases: Vec<_> = cases
        .iter()
        .filter(|c| c.id.starts_with("sci-xf-bounds-"))
        .collect();
    assert_eq!(cases.len(), 61);
    for case in cases {
        assert_eq!(
            native::run(case.source, case.profile).unwrap_or_else(|e| panic!("{}: {e}", case.id)),
            native::Observation::expected(case),
            "{}: {}",
            case.id,
            case.source
        );
    }
}

#[test]
fn native_cat_and_mapcat_match_pinned_sci() {
    let cases = protocol::cases(include_str!("fixtures/sci-outcomes.tsv")).unwrap();
    let cases: Vec<_> = cases
        .iter()
        .filter(|c| c.id.starts_with("sci-xf-cat-"))
        .collect();
    assert_eq!(cases.len(), 47);
    for case in cases {
        assert_eq!(
            native::run(case.source, case.profile).unwrap_or_else(|e| panic!("{}: {e}", case.id)),
            native::Observation::expected(case),
            "{}: {}",
            case.id,
            case.source
        );
    }
}

#[test]
fn native_partition_all_matches_pinned_sci() {
    let cases = protocol::cases(include_str!("fixtures/sci-outcomes.tsv")).unwrap();
    let cases: Vec<_> = cases
        .iter()
        .filter(|c| c.id.starts_with("sci-xf-partition-"))
        .collect();
    assert_eq!(cases.len(), 58);
    for case in cases {
        assert_eq!(
            native::run(case.source, case.profile).unwrap_or_else(|e| panic!("{}: {e}", case.id)),
            native::Observation::expected(case),
            "{}: {}",
            case.id,
            case.source
        );
    }
}

#[test]
fn partition_buffers_are_independent_rooted_and_not_reset_by_initialization() {
    let mut c = Context::new();
    let xf = c.eval_string("(partition-all 2)").unwrap();
    let rf = c.eval_string("(completing conj)").unwrap();
    let a = c.invoke(&xf, std::slice::from_ref(&rf)).unwrap();
    let b = c.invoke(&xf, &[rf]).unwrap();
    drop(xf);
    let first = c.eval_string("(let [kept [42]] (fn [] kept))").unwrap();
    let a1 = c.invoke(&a, &[c.vector(&[]).unwrap(), first]).unwrap();
    let b1 = c
        .invoke(&b, &[c.vector(&[]).unwrap(), c.integer(4)])
        .unwrap();
    c.collect_garbage();
    assert_eq!(c.memory_stats().executable_units, 1);
    assert_eq!(c.invoke(&a, &[]).unwrap().count(), Some(0));
    let a2 = c.invoke(&a, &[a1, c.integer(2)]).unwrap();
    c.collect_garbage();
    let group = a2.nth(0).unwrap().unwrap();
    assert_eq!(group.count(), Some(2));
    let kept = c.invoke(&group.nth(0).unwrap().unwrap(), &[]).unwrap();
    assert_eq!(
        kept.nth(0).unwrap().unwrap().inspect(),
        ValueView::Integer(42)
    );
    let b2 = c.invoke(&b, &[b1]).unwrap();
    c.collect_garbage();
    assert_eq!(
        b2.nth(0)
            .unwrap()
            .unwrap()
            .nth(0)
            .unwrap()
            .unwrap()
            .inspect(),
        ValueView::Integer(4)
    );
    assert_eq!(
        c.invoke(&b, &[c.vector(&[]).unwrap()]).unwrap().count(),
        Some(0)
    );
    assert!(matches!(
        c.eval_string("(partition-all nil)").unwrap_err().kind,
        EvalErrorKind::Native {
            error: NativeError::Type { .. },
            ..
        }
    ));
}

#[test]
fn interleaved_counted_reducers_keep_state_through_gc_and_initialization() {
    for (constructor, count, expected_a, expected_b) in [
        ("take", 2, vec![1, 2], vec![4, 5]),
        ("drop", 1, vec![2], vec![5]),
    ] {
        let mut c = Context::new();
        let xf = c.eval_string(&format!("({constructor} {count})")).unwrap();
        let conj = c.eval_string("conj").unwrap();
        let a = c.invoke(&xf, std::slice::from_ref(&conj)).unwrap();
        let b = c.invoke(&xf, &[conj]).unwrap();
        drop(xf);
        c.collect_garbage();
        let a1 = c
            .invoke(&a, &[c.vector(&[]).unwrap(), c.integer(1)])
            .unwrap();
        c.collect_garbage();
        let b1 = c
            .invoke(&b, &[c.vector(&[]).unwrap(), c.integer(4)])
            .unwrap();
        c.collect_garbage();
        assert_eq!(c.invoke(&a, &[]).unwrap().count(), Some(0));
        c.collect_garbage();
        let a2 = c.invoke(&a, &[a1, c.integer(2)]).unwrap();
        c.collect_garbage();
        let b2 = c.invoke(&b, &[b1, c.integer(5)]).unwrap();
        c.collect_garbage();
        assert_eq!(a2.reduced_value().is_some(), constructor == "take");
        assert_eq!(b2.reduced_value().is_some(), constructor == "take");
        for (rf, value, expected) in [(a, a2, expected_a), (b, b2, expected_b)] {
            let value = value.reduced_value().unwrap_or(value);
            let completed = c.invoke(&rf, &[value]).unwrap();
            c.collect_garbage();
            assert_eq!(
                completed
                    .elements()
                    .unwrap()
                    .iter()
                    .map(|v| v.inspect())
                    .collect::<Vec<_>>(),
                expected
                    .into_iter()
                    .map(ValueView::Integer)
                    .collect::<Vec<_>>()
            );
        }
    }
}

thread_local! {
    static EVENTS: RefCell<Vec<i64>> = const { RefCell::new(Vec::new()) };
    static FAULT: RefCell<Option<(i64, bool)>> = const { RefCell::new(None) };
    static SAMPLES: RefCell<Vec<(usize, u64)>> = const { RefCell::new(Vec::new()) };
}
static BOUNDARY: NativeDefinition = NativeDefinition {
    metadata: NativeMetadata {
        namespace: "host",
        name: "boundary",
        arity: Arity::Exact(1),
        effects: NativeEffects::Host,
        errors: &[],
    },
    callback: |c, args| {
        let ValueView::Integer(stage) = args[0].inspect() else {
            panic!("stage")
        };
        EVENTS.with(|e| e.borrow_mut().push(stage));
        if let Some((_, reset)) = FAULT.with(|f| *f.borrow()).filter(|(s, _)| *s == stage) {
            if reset {
                c.reset();
                return Ok(c.nil());
            }
            return Ok(Context::new().nil());
        }
        let before = c.resource_usage();
        assert_eq!(
            c.eval_string("(+ 20 22)").unwrap().inspect(),
            ValueView::Integer(42)
        );
        let after = c.resource_usage();
        assert!(after.fuel > before.fuel);
        assert!(after.allocation >= before.allocation);
        c.collect_garbage();
        assert_eq!(c.resource_usage(), after);
        SAMPLES.with(|s| s.borrow_mut().push((c.memory_stats().roots, after.fuel)));
        Ok(args[0].clone())
    },
};

const PROCESS: &str = r#"
    (let [kept ["captured"]
          rf (fn ([] (host/boundary 0) kept)
                 ([a] (host/boundary 4) a)
                 ([a x] (host/boundary 3) (conj a x)))
          xf (fn [rf] (host/boundary 1)
               (let [n (volatile! 0)]
                 (fn ([] (rf)) ([a] (rf a))
                     ([a x] (vswap! n inc) (rf a [kept @n x])))))]
      (transduce xf rf (lazy-seq (host/boundary 2) (list 5 6))))
"#;

const INTO_PROCESS: &str = r#"
    (let [kept ["captured"]
          target (with-meta ["target"] {:kept kept})
          xf (fn [rf] (host/boundary 1)
               (let [n (volatile! 0)]
                 (fn ([a] (host/boundary 4) (rf a))
                     ([a x] (host/boundary 3) (vswap! n inc)
                      (rf a [kept @n x])))))
          result (into target xf (lazy-seq (host/boundary 2) (list 5 6)))]
      [(= result ["target" [["captured"] 1 5] [["captured"] 2 6]])
       (= (meta result) {:kept kept}) (= target ["target"])])
"#;

const NATIVE_PROCESS: &str = r#"
    (let [kept ["captured"]
          rf (fn ([] (host/boundary 0) [kept])
                 ([a] (host/boundary 6) a)
                 ([a x] (host/boundary 5) (conj a x)))
          xf (comp (map (fn [x] (host/boundary 2) [kept x]))
                   (filter (fn [x] (host/boundary 3) (= (first x) kept)))
                   (remove (fn [x] (host/boundary 4) false)))]
      (= (transduce xf rf (lazy-seq (host/boundary 1) (list 5)))
         [kept [kept 5]]))
"#;

const BOUNDED_PROCESS: &str = r#"
    (let [kept ["captured"]
          rf (fn ([] (host/boundary 0) [kept])
                 ([a] (host/boundary 4) a)
                 ([a x] (host/boundary 3) (conj a x)))
          xf (comp (drop 1)
                   (take-while (fn [x] (host/boundary 2) (= (first x) kept)))
                   (take 1))]
      (= (transduce xf rf (lazy-seq (host/boundary 1) (list nil [kept 5])))
         [kept [kept 5]]))
"#;

const PARTITION_PROCESS: &str = r#"
    (let [kept ["captured"]
          rf (fn ([] (host/boundary 0) [kept])
                 ([a] (host/boundary 5) a)
                 ([a x] (host/boundary (if (= 2 (count x)) 3 4)) (conj a x)))
          xf (comp (map (fn [x] (host/boundary 2) [kept x])) (partition-all 2))]
      (= (transduce xf rf (lazy-seq (host/boundary 1) (list 1 2 3)))
         [kept [[kept 1] [kept 2]] [[kept 3]]]))
"#;

#[test]
fn partition_buffers_survive_callbacks_and_stop_on_owner_changes() {
    let expected = [0, 1, 2, 2, 3, 2, 4, 5];
    let mut c = Context::new();
    c.register_native(&BOUNDARY);
    FAULT.with(|f| *f.borrow_mut() = None);
    EVENTS.with(|e| e.borrow_mut().clear());
    assert_eq!(
        c.eval_string(PARTITION_PROCESS).unwrap().inspect(),
        ValueView::Boolean(true)
    );
    EVENTS.with(|e| assert_eq!(*e.borrow(), expected));
    c.collect_garbage();
    assert_eq!(c.memory_stats().executable_units, 0);
    for reset in [false, true] {
        for stage in 0..=5 {
            let mut c = Context::new();
            c.register_native(&BOUNDARY);
            FAULT.with(|f| *f.borrow_mut() = Some((stage, reset)));
            EVENTS.with(|e| e.borrow_mut().clear());
            let error = c.eval_string(PARTITION_PROCESS).unwrap_err();
            assert!(error.to_string().contains("WrongOwner"), "{error}");
            let stop = expected.iter().position(|s| *s == stage).unwrap() + 1;
            EVENTS.with(|e| assert_eq!(*e.borrow(), expected[..stop]));
            c.collect_garbage();
        }
    }
    FAULT.with(|f| *f.borrow_mut() = None);
}

#[test]
fn partition_emission_and_reentrant_completion_keep_roots_bounded() {
    let mut c = Context::new();
    c.register_native(&BOUNDARY);
    FAULT.with(|f| *f.borrow_mut() = None);
    SAMPLES.with(|s| s.borrow_mut().clear());
    assert_eq!(c.eval_string("(transduce (partition-all 3) (completing (fn [a xs] (host/boundary 3) (+ a (reduce + xs)))) 0 (range 3000))").unwrap().inspect(), ValueView::Integer(4_498_500));
    SAMPLES.with(|s| {
        let samples = s.borrow();
        assert_eq!(samples.len(), 1000);
        let low = samples[10..].iter().map(|s| s.0).min().unwrap();
        let high = samples[10..].iter().map(|s| s.0).max().unwrap();
        assert!(high - low <= 2, "partition roots grew: {low}..{high}");
    });
    assert_eq!(c.eval_string("(let [cell (volatile! nil) once (volatile! true) r ((partition-all 3) (fn ([a] (host/boundary 5) a) ([a x] (host/boundary 3) (if @once (do (vreset! once false) (@cell a)) nil) (conj a x))))] (vreset! cell r) (= (r (r [] [\"kept\"])) [[[\"kept\"]]]))").unwrap().inspect(), ValueView::Boolean(true));
}

const CAT_PROCESS: &str = r#"
    (let [kept ["captured"]
          rf (fn ([] (host/boundary 0) [kept])
                 ([a] (host/boundary 5) a)
                 ([a x] (host/boundary 4) (reduced (reduced [a x]))))
          xf (comp (mapcat (fn [x] (host/boundary 2) x)) cat)
          result (transduce xf rf
                   (lazy-seq (host/boundary 1)
                     (list (list (lazy-seq (host/boundary 3)
                                   (cons [kept 7]
                                     (lazy-seq (host/boundary 99) (list 8))))))))]
      (and (reduced? result) (= @result [[kept] [kept 7]])))
"#;

#[test]
fn nested_cat_roots_callbacks_and_markers_across_collection_reentry_and_reset() {
    let mut c = Context::new();
    c.register_native(&BOUNDARY);
    FAULT.with(|f| *f.borrow_mut() = None);
    EVENTS.with(|e| e.borrow_mut().clear());
    assert_eq!(
        c.eval_string(CAT_PROCESS).unwrap().inspect(),
        ValueView::Boolean(true)
    );
    EVENTS.with(|e| assert_eq!(*e.borrow(), [0, 1, 2, 3, 4, 5]));
    c.collect_garbage();
    assert_eq!(c.memory_stats().executable_units, 0);
    for reset in [false, true] {
        for stage in 0..=5 {
            let mut c = Context::new();
            c.register_native(&BOUNDARY);
            FAULT.with(|f| *f.borrow_mut() = Some((stage, reset)));
            EVENTS.with(|e| e.borrow_mut().clear());
            let error = c.eval_string(CAT_PROCESS).unwrap_err();
            assert!(error.to_string().contains("WrongOwner"), "{error}");
            EVENTS.with(|e| assert_eq!(*e.borrow(), (0..=stage).collect::<Vec<_>>()));
            c.collect_garbage();
        }
    }
    FAULT.with(|f| *f.borrow_mut() = None);
}

#[test]
fn nested_cat_keeps_scratch_roots_and_stack_bounded() {
    let mut c = Context::new();
    assert_eq!(
        c.eval_string("(transduce (comp cat cat) (completing +) 0 [[(range 100000)]])")
            .unwrap()
            .inspect(),
        ValueView::Integer(4_999_950_000)
    );
    c.register_native(&BOUNDARY);
    FAULT.with(|f| *f.borrow_mut() = None);
    SAMPLES.with(|s| s.borrow_mut().clear());
    assert_eq!(c.eval_string("(transduce (comp (mapcat (fn [x] [[x]])) cat) (completing (fn [a x] (host/boundary 3) (+ a x))) 0 (range 1000))").unwrap().inspect(), ValueView::Integer(499_500));
    SAMPLES.with(|s| {
        let samples = s.borrow();
        assert_eq!(samples.len(), 1000);
        let low = samples[10..].iter().map(|s| s.0).min().unwrap();
        let high = samples[10..].iter().map(|s| s.0).max().unwrap();
        assert!(high - low <= 2, "cat roots grew: {low}..{high}");
    });
}

#[test]
fn bounded_adapters_trace_state_and_stop_on_callback_owner_changes() {
    let mut c = Context::new();
    c.register_native(&BOUNDARY);
    FAULT.with(|f| *f.borrow_mut() = None);
    EVENTS.with(|e| e.borrow_mut().clear());
    assert_eq!(
        c.eval_string(BOUNDED_PROCESS).unwrap().inspect(),
        ValueView::Boolean(true)
    );
    EVENTS.with(|e| assert_eq!(*e.borrow(), [0, 1, 2, 3, 4]));
    c.collect_garbage();
    assert_eq!(c.memory_stats().executable_units, 0);
    for reset in [false, true] {
        for stage in 0..=4 {
            let mut c = Context::new();
            c.register_native(&BOUNDARY);
            FAULT.with(|f| *f.borrow_mut() = Some((stage, reset)));
            EVENTS.with(|e| e.borrow_mut().clear());
            let error = c.eval_string(BOUNDED_PROCESS).unwrap_err();
            assert!(error.to_string().contains("WrongOwner"), "{error}");
            EVENTS.with(|e| assert_eq!(*e.borrow(), (0..=stage).collect::<Vec<_>>()));
            c.collect_garbage();
        }
    }
    FAULT.with(|f| *f.borrow_mut() = None);
}

#[test]
fn native_adapters_trace_captures_and_stop_on_callback_owner_changes() {
    let mut c = Context::new();
    c.register_native(&BOUNDARY);
    FAULT.with(|f| *f.borrow_mut() = None);
    EVENTS.with(|e| e.borrow_mut().clear());
    assert_eq!(
        c.eval_string(NATIVE_PROCESS).unwrap().inspect(),
        ValueView::Boolean(true)
    );
    EVENTS.with(|e| assert_eq!(*e.borrow(), [0, 1, 2, 3, 4, 5, 6]));
    c.collect_garbage();
    assert_eq!(c.memory_stats().executable_units, 0);

    for reset in [false, true] {
        for stage in 0..=6 {
            let mut c = Context::new();
            c.register_native(&BOUNDARY);
            FAULT.with(|f| *f.borrow_mut() = Some((stage, reset)));
            EVENTS.with(|e| e.borrow_mut().clear());
            let error = c.eval_string(NATIVE_PROCESS).unwrap_err();
            assert!(error.to_string().contains("WrongOwner"), "{error}");
            EVENTS.with(|e| assert_eq!(*e.borrow(), (0..=stage).collect::<Vec<_>>()));
            c.collect_garbage();
        }
    }
    FAULT.with(|f| *f.borrow_mut() = None);
}

#[test]
fn into_retains_target_metadata_accumulator_and_transform_across_collection() {
    let mut c = Context::new();
    c.register_native(&BOUNDARY);
    FAULT.with(|f| *f.borrow_mut() = None);
    EVENTS.with(|e| e.borrow_mut().clear());
    let result = c.eval_string(INTO_PROCESS).unwrap();
    assert!(
        result
            .elements()
            .unwrap()
            .iter()
            .all(|v| v.inspect() == ValueView::Boolean(true))
    );
    EVENTS.with(|e| assert_eq!(*e.borrow(), [1, 2, 3, 3, 4]));
    drop(result);
    c.collect_garbage();
    assert_eq!(c.memory_stats().executable_units, 0);

    for reset in [false, true] {
        for stage in 1..=4 {
            let mut c = Context::new();
            c.register_native(&BOUNDARY);
            FAULT.with(|f| *f.borrow_mut() = Some((stage, reset)));
            EVENTS.with(|e| e.borrow_mut().clear());
            let error = c.eval_string(INTO_PROCESS).unwrap_err();
            assert!(error.to_string().contains("WrongOwner"), "{error}");
            let expected = if stage == 4 {
                vec![1, 2, 3, 3, 4]
            } else {
                (1..=stage).collect()
            };
            EVENTS.with(|e| assert_eq!(*e.borrow(), expected));
            c.collect_garbage();
        }
    }
    FAULT.with(|f| *f.borrow_mut() = None);
}

#[test]
fn callbacks_reenter_and_collect_at_every_process_boundary() {
    let mut c = Context::new();
    c.register_native(&BOUNDARY);
    FAULT.with(|f| *f.borrow_mut() = None);
    EVENTS.with(|e| e.borrow_mut().clear());
    let result = c.eval_string(PROCESS).unwrap();
    assert_eq!(
        native::wire(&mut c, &result).unwrap().wire(),
        "(vector (string u00630061007000740075007200650064) (vector (vector (string u00630061007000740075007200650064)) (int 1) (int 5)) (vector (vector (string u00630061007000740075007200650064)) (int 2) (int 6)))"
    );
    EVENTS.with(|e| assert_eq!(*e.borrow(), [0, 1, 2, 3, 3, 4]));
    drop(result);
    c.collect_garbage();
    assert_eq!(c.memory_stats().executable_units, 0);
}

#[test]
fn reset_and_foreign_callback_results_stop_before_completion() {
    for reset in [false, true] {
        for stage in 0..=4 {
            let mut c = Context::new();
            c.register_native(&BOUNDARY);
            FAULT.with(|f| *f.borrow_mut() = Some((stage, reset)));
            EVENTS.with(|e| e.borrow_mut().clear());
            let error = c.eval_string(PROCESS).unwrap_err();
            assert!(error.to_string().contains("WrongOwner"), "{error}");
            let expected = if stage == 4 {
                vec![0, 1, 2, 3, 3, 4]
            } else {
                (0..=stage).collect()
            };
            EVENTS.with(|e| assert_eq!(*e.borrow(), expected));
            c.collect_garbage();
        }
    }
    FAULT.with(|f| *f.borrow_mut() = None);
}

#[test]
fn long_processes_keep_stack_and_scratch_roots_bounded() {
    let mut c = Context::new();
    assert_eq!(
        c.eval_string("(transduce (fn [rf] rf) (completing +) 0 (range 100000))")
            .unwrap()
            .inspect(),
        ValueView::Integer(4_999_950_000)
    );
    c.register_native(&BOUNDARY);
    FAULT.with(|f| *f.borrow_mut() = None);
    SAMPLES.with(|s| s.borrow_mut().clear());
    c.eval_string(
        "(transduce identity (completing (fn [a x] (host/boundary 3) (+ a x))) 0 (range 1000))",
    )
    .unwrap();
    SAMPLES.with(|s| {
        let samples = s.borrow();
        assert_eq!(samples.len(), 1000);
        let steady = &samples[10..];
        let low = steady.iter().map(|s| s.0).min().unwrap();
        let high = steady.iter().map(|s| s.0).max().unwrap();
        assert!(high - low <= 2, "roots grew: {low}..{high}");
        assert!(steady.windows(2).all(|s| s[1].1 > s[0].1));
    });

    assert_eq!(c.eval_string("(transduce (comp (drop 1) (take-while #(< % 100000)) (take 100000)) (completing +) 0 (range))").unwrap().inspect(), ValueView::Integer(4_999_950_000));
    SAMPLES.with(|s| s.borrow_mut().clear());
    assert_eq!(c.eval_string("(transduce (comp (drop 1) (take-while (fn [x] (host/boundary 3) true)) (take 1000)) (completing +) 0 (range))").unwrap().inspect(), ValueView::Integer(500_500));
    SAMPLES.with(|s| {
        let samples = s.borrow();
        assert_eq!(samples.len(), 1000);
        let low = samples[10..].iter().map(|s| s.0).min().unwrap();
        let high = samples[10..].iter().map(|s| s.0).max().unwrap();
        assert!(high - low <= 2, "counted reducer roots grew: {low}..{high}");
    });
}

#[test]
fn direct_native_and_adapter_invocations_keep_exception_source_and_owner_checks() {
    let mut c = Context::new();
    let completing = c.eval_string("completing").unwrap();
    let step = c.eval_string("(fn [a x] (+ a x))").unwrap();
    let adapter = c.invoke(&completing, &[step]).unwrap();
    assert_eq!(
        c.invoke(&adapter, &[c.integer(1), c.integer(2)])
            .unwrap()
            .inspect(),
        ValueView::Integer(3)
    );
    let transduce = c.eval_string("transduce").unwrap();
    let identity = c.eval_string("identity").unwrap();
    let source = c.source(
        "transducer-error.clj",
        "(fn [a x] (throw (ex-info \"step\" {})))",
    );
    let form = c.reader(&source).unwrap().read_form().unwrap().unwrap();
    let failing = c.eval_form(&form).unwrap();
    let error = c
        .invoke(
            &transduce,
            &[
                identity,
                failing,
                c.nil(),
                c.vector(&[c.integer(1)]).unwrap(),
            ],
        )
        .unwrap_err();
    let InvokeError::Execution(error) = error else {
        panic!("source-backed failure")
    };
    assert_eq!(error.span.source().name(), "transducer-error.clj");
    assert_eq!(error.span.text(), "(throw (ex-info \"step\" {}))");
    assert_eq!(
        error.kind,
        EvalErrorKind::Thrown(ExceptionCategory::ExceptionInfo)
    );
    let foreign = Context::new();
    assert!(matches!(
        c.invoke(&adapter, &[foreign.nil()]),
        Err(InvokeError::Dispatch(EvalErrorKind::WrongOwner))
    ));
}
