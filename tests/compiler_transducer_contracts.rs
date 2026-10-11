//! Canonical ABI, compile-time execution and engine lifetime contracts for eager
//! transduction. CLJS value/effect oracle cases live in compiler_transducers.
use light_cljs::evaluator::{Evaluator, EvaluatorOptions, RUNTIME_SOURCE};
use light_cljs::{MacroHost, analyze_with_macros, emitter::emit_js_unit, parse};
use std::process::Command;

fn program(source: &str) -> light_cljs::Program {
    let mut host = MacroHost::new();
    host.disable_native_evaluation();
    analyze_with_macros(source, &parse(source).unwrap(), &mut host).unwrap()
}
fn both(source: &str, expected: &str) {
    let p = program(source);
    let e = Evaluator::new(EvaluatorOptions::default()).unwrap();
    assert_eq!(
        e.print(e.evaluate(&p).unwrap()).unwrap(),
        expected,
        "QuickJS: {source}"
    );
    let script = format!(
        "const rt = {RUNTIME_SOURCE}; console.log(rt.core.prStr(({})(rt)));",
        emit_js_unit(&p)
    );
    let out = Command::new("node").args(["-e", &script]).output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8(out.stdout).unwrap().trim(),
        expected,
        "Node: {source}"
    );
}

#[test]
fn macros_transduce_in_canonical_js_and_export_persistent_forms() {
    both(
        r#"
      (defmacro generated []
        (into [] (comp (filter odd?) (map inc) (take 3)) (range)))
      (defmacro groups [] (into [] (partition-all 3) (range 5)))
      (defmacro total [] (transduce (comp (filter odd?) (map inc) (take 3)) + 0 (range)))
      [(generated) (groups) (total)]"#,
        "[[2 4 6] [[0 1 2] [3 4]] 12]",
    );
}

#[test]
fn ifn_and_ireduce_use_canonical_dispatch_and_complete_once() {
    both(
        r#"
      (let [events (atom [])
            mapper (reify IFn (-invoke [_ x] (swap! events conj [:map x]) (inc x)))
            reducer (reify IFn
                      (-invoke [_] (swap! events conj :init) [])
                      (-invoke [_ a] (swap! events conj :complete) a)
                      (-invoke [_ a x] (swap! events conj [:step x]) (conj a x)))
            source (reify IReduce
                     (-reduce [_ rf init]
                       (swap! events conj :reduce)
                       (loop [a init s [1 2 3 4]]
                         (if (seq s)
                           (let [ret (rf a (first s))]
                             (if (reduced? ret) @ret (recur ret (next s)))) a))))
            result (transduce (comp (map mapper) (take 2)) reducer source)]
        [result @events])"#,
        "[[2 3] [:init :reduce [:map 1] [:step 2] [:map 2] [:step 3] :complete]]",
    );
    both(
        r#"
      (let [dest (reify ICollection
                   (-conj [_ x] (list :custom x)))]
        (into dest (map inc) [1]))"#,
        "(:custom 2)",
    );
}

#[test]
fn state_and_captures_survive_collection_and_failed_reentrant_completion() {
    both(
        r#"
      (let [cell (atom nil) once (atom true) calls (atom [])
            rf ((partition-all 3)
                 (fn ([a] (swap! calls conj :complete) a)
                     ([a x] (swap! calls conj :emit)
                       (when @once (reset! once false) (@cell a))
                       (conj a x))))]
        (reset! cell rf)
        [(rf (rf [] ["kept"])) @calls])"#,
        "[[[[\"kept\"]]] [:emit :complete :complete]]",
    );
    // This calls the public core directly without a compiler helper or Rust
    // callbacks per element, and forces GC while two reducer buffers coexist.
    let js = r#"(() => {
      const c = rt.core, xf = c.partitionAll(2), rf = c.completing(c.conj);
      const a = xf(rf), b = xf(rf), captured = c.vector(42);
      let aa = a(c.vector(), () => captured), bb = b(c.vector(), 4);
      if (typeof gc === 'function') gc();
      a(); // initialization forwards; it does not reset the buffered input.
      aa = a(aa, 2); bb = b(bb);
      return c.vector(c.first(c.first(aa))(), c.nth(c.first(aa), 1), bb);
    })()"#;
    let e = Evaluator::new(EvaluatorOptions::default()).unwrap();
    assert_eq!(
        e.print(e.evaluate_js(js).unwrap()).unwrap(),
        "[[42] 2 [[4]]]"
    );
    let script = format!("const rt = {RUNTIME_SOURCE}; console.log(rt.core.prStr({js}));");
    let out = Command::new("node")
        .args(["--expose-gc", "-e", &script])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8(out.stdout).unwrap().trim(),
        "[[42] 2 [[4]]]"
    );
}

#[test]
fn cancellation_at_each_boundary_stops_exactly_and_keeps_session_terminal() {
    use std::sync::{Arc, Mutex};
    let source = r#"
      (let [rf (fn ([] (js/eventTransduction "init") [])
                   ([a] (js/eventTransduction "complete") a)
                   ([a x] (js/eventTransduction "step") (conj a x)))
            xf (fn [rf] (js/eventTransduction "transform") ((map inc) rf))]
        (transduce xf rf (lazy-seq (js/eventTransduction "source") (list 1 2))))"#;
    let p = program(source);
    let expected = ["init", "transform", "source", "step", "step", "complete"];
    for stage in ["init", "transform", "source", "step", "complete"] {
        let e = Evaluator::new(EvaluatorOptions::default()).unwrap();
        let events = Arc::new(Mutex::new(Vec::new()));
        let captured = events.clone();
        e.install_callback("eventTransduction", move |control, event| {
            captured.lock().unwrap().push(event.clone());
            if event == stage {
                control.cancel();
            }
            Ok(event)
        })
        .unwrap();
        let error = e.evaluate(&p).unwrap_err().to_string();
        assert!(error.contains("cancelled"), "{stage}: {error}");
        let stop = expected.iter().position(|event| *event == stage).unwrap() + 1;
        assert_eq!(*events.lock().unwrap(), expected[..stop], "{stage}");
        assert!(
            e.evaluate_js("42")
                .unwrap_err()
                .to_string()
                .contains("cancelled")
        );
    }
}

#[test]
fn long_nested_transduction_uses_bounded_stack() {
    both(
        "(transduce (comp cat cat (map inc)) (completing +) 0 [[(range 10000)]])",
        "50005000",
    );
    both(
        "(transduce (comp (drop 1) (take-while #(< % 10000)) (take 10000)) (completing +) 0 (range))",
        "49995000",
    );
}

#[test]
fn random_sampling_observes_redefined_rand_and_lazy_collection_demand() {
    both(
        r#"
      (let [samples (atom [0.1 0.8 0.2 0.9]) calls (atom 0) xf (random-sample 0.5)]
        (with-redefs [rand (fn [] (swap! calls inc)
                            (let [n (first @samples)] (swap! samples rest) n))]
          [(into [] xf [1 2 3 4]) @calls]))"#,
        "[[1 3] 4]",
    );
    both(
        r#"
      (let [calls (atom 0)]
        (with-redefs [rand (fn [] (swap! calls inc) 0)]
          (let [xs (random-sample 0.5 (list 1 2 3)) before @calls]
            [before (first xs) @calls])))"#,
        "[0 1 1]",
    );
}
