//! Canonical sequence ABI, shared macro/application execution, cancellation and GC.
use light_cljs::evaluator::{Evaluator, EvaluatorOptions, RUNTIME_SOURCE};
use light_cljs::{MacroHost, analyze_with_macros, emitter::emit_js_unit, parse};
use std::process::Command;

fn program(source: &str) -> light_cljs::Program {
    analyze_with_macros(source, &parse(source).unwrap(), &mut MacroHost::new()).unwrap()
}
fn both(source: &str, expected: &str) {
    let p = program(source);
    let e = Evaluator::new(EvaluatorOptions::default()).unwrap();
    assert_eq!(e.print(e.evaluate(&p).unwrap()).unwrap(), expected);
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
    assert_eq!(String::from_utf8(out.stdout).unwrap().trim(), expected);
}

#[test]
fn sequence_public_arities_run_as_shared_macro_and_application_helpers() {
    both(
        r#"
      (defn sequence-witness []
        (let [saved cljs.core/sequence
              calls (atom 0)
              lazy-input (lazy-seq (swap! calls inc) (list 9))
              xf (comp (map-indexed vector) (take 2))
              a (saved xf (range)) b (saved xf (range 10 20))]
          (assert (identical? lazy-input (saved lazy-input)))
          (assert (= 0 @calls))
          [(vec (saved nil))
           (vec (saved (comp (filter odd?) (map inc) (take 3)) (range)))
           (vec (saved (partition-all 3) (range 5)))
           (vec (saved (map +) [1 2] [10 20]))
           (vec (apply saved (map +) [[1 2] [3 4] [5 6] [7 8]]))
           (first a) (first b) (vec a) (vec b) (vec a)]))
      (defmacro generated [] (sequence-witness))
      [(sequence-witness) (generated)]"#,
        "[[[] [2 4 6] [[0 1 2] [3 4]] [11 22] [16 20] [0 0] [0 10] [[0 0] [1 1]] [[0 10] [1 11]] [[0 0] [1 1]]] [[] [2 4 6] [[0 1 2] [3 4]] [11 22] [16 20] [0 0] [0 10] [[0 0] [1 1]] [[0 10] [1 11]] [[0 0] [1 1]]]]",
    );
}

#[test]
fn user_iterators_and_callable_transforms_preserve_source_order() {
    both(
        r#"
      (let [events (atom []) n (atom 0)
            source (reify IIterable
                     (-iterator [_]
                       (swap! events conj :iterator)
                       (reify Object
                         (hasNext [_] (swap! events conj [:has @n]) (< @n 3))
                         (next [_] (let [i @n] (swap! events conj [:next i]) (swap! n inc) i)))))
            mapper (reify IFn (-invoke [_ x] (swap! events conj [:map x]) (inc x)))
            s (sequence (comp (map mapper) (take 2)) source)
            before @events]
        [before (vec s) @events (vec s) @events])"#,
        "[[:iterator] [1 2] [:iterator [:has 0] [:next 0] [:map 0] [:has 1] [:next 1] [:map 1]] [1 2] [:iterator [:has 0] [:next 0] [:map 0] [:has 1] [:next 1] [:map 1]]]",
    );
}

#[test]
fn cancellation_in_sequence_construction_demand_and_completion_is_terminal() {
    use std::sync::{Arc, Mutex};
    let p = program(
        r#"
      (let [xf (fn [rf] (js/eventSequence "transform")
                 (fn ([a] (js/eventSequence "completion") (rf a))
                     ([a x] (js/eventSequence "step") (rf a x))))
            s (sequence xf (lazy-seq (js/eventSequence "source") (list 1 2)))]
        (vec s))"#,
    );
    let expected = ["transform", "source", "step", "step", "completion"];
    for stage in ["transform", "source", "step", "completion"] {
        let e = Evaluator::new(EvaluatorOptions::default()).unwrap();
        let events = Arc::new(Mutex::new(Vec::new()));
        let captured = events.clone();
        e.install_callback("eventSequence", move |control, event| {
            captured.lock().unwrap().push(event.clone());
            if event == stage {
                control.cancel();
            }
            Ok(event)
        })
        .unwrap();
        assert!(
            e.evaluate(&p)
                .unwrap_err()
                .to_string()
                .contains("cancelled"),
            "{stage}"
        );
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
fn abandoned_prefixes_and_reducer_cycles_are_collected_in_both_engines() {
    let batch = r#"(() => {
      const c = rt.core;
      for (let i=0; i<500; i++) {
        const capture = new Array(1000).fill(i);
        const holder = {};
        const xf = rf => {
          const f = function(a, x) { return arguments.length===1 ? rf(a) : rf(a, capture[0]+x); };
          holder.reducer = f; f.holder = holder; return f;
        };
        const s = c.sequence(xf, c.range());
        if (c.first(s)!==i) throw new Error('capture lost');
        if (i%50===0) gc();
      }
      gc(); return 42;
    })()"#;
    let e = Evaluator::new(EvaluatorOptions {
        memory_limit: 8 * 1024 * 1024,
        timeout: std::time::Duration::from_secs(60),
        ..EvaluatorOptions::default()
    })
    .unwrap();
    e.install_gc_callback("gc").unwrap();
    e.collect_garbage();
    let before = e.memory_usage_bytes();
    for _ in 0..3 {
        let result = e.evaluate_js(batch).unwrap();
        assert_eq!(e.number(result).unwrap(), 42.0);
        e.release(result).unwrap();
        e.collect_garbage();
        let after = e.memory_usage_bytes();
        assert!(
            after < before + 128 * 1024,
            "retained JS heap: {before} -> {after}"
        );
    }
    let script = format!("const rt = {RUNTIME_SOURCE}; for(let i=0;i<3;i++) console.log({batch});");
    let out = Command::new("node")
        .args(["--expose-gc", "--max-old-space-size=32", "-e", &script])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.stdout, b"42\n42\n42\n");
}
