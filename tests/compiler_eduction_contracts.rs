//! Canonical eduction ABI, shared macro/application execution, cancellation and GC.
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
fn eduction_public_arities_run_as_shared_macro_and_application_helpers() {
    both(
        r#"
      (defn eduction-witness []
        (let [saved cljs.core/eduction
              calls (atom [])
              xf (map #(do (swap! calls conj %) (inc %)))
              e (saved xf (take 2) [1 2 3])
              before @calls a (into [] e) middle @calls b (into [] e)
              direct (Eduction. (partition-all 3) (range 5))
              view (saved (map-indexed vector) (take 3) (range))
              i (-iterator view) j (-iterator view)]
          (assert (= [] before))
          (assert (= [1 2] middle))
          (assert (= [1 2 1 2] @calls))
          (assert (instance? Eduction direct))
          (assert (and (satisfies? IReduce direct) (satisfies? ISeqable direct)
                       (satisfies? IIterable direct) (sequential? direct)))
          (assert (nil? (meta direct)))
          [(vec (saved)) (vec (saved nil)) (vec (saved [9])) a b
           (vec (apply saved [(filter odd?) (map inc) (take 3) (range)]))
           (vec direct) (vec direct) (reduce + (saved (map inc) [1 2]))
           (-reduce (saved (map inc) [1 2]) + 10)
           (.next i) (.next j) (.next i) (.next j)]))
      (defmacro generated [] (eduction-witness))
      [(eduction-witness) (generated)]"#,
        "[[[] [] [9] [2 3] [2 3] [2 4 6] [[0 1 2] [3 4]] [[0 1 2] [3 4]] 5 15 [0 0] [0 0] [1 1] [1 1]] [[] [] [9] [2 3] [2 3] [2 4 6] [[0 1 2] [3 4]] [[0 1 2] [3 4]] 5 15 [0 0] [0 0] [1 1] [1 1]]]",
    );
}

#[test]
fn throwing_application_and_macro_views_recover_in_fresh_sessions() {
    let p = program("(into [] (eduction (map #(throw (js/Error. \"view failed\"))) [1 2]))");
    let e = Evaluator::new(EvaluatorOptions::default()).unwrap();
    assert!(
        e.evaluate(&p)
            .unwrap_err()
            .to_string()
            .contains("view failed")
    );
    let source = "(defmacro broken [] (vec (eduction (map #(throw (js/Error. \"macro view failed\"))) [1 2]))) (broken)";
    let error =
        analyze_with_macros(source, &parse(source).unwrap(), &mut MacroHost::new()).unwrap_err();
    assert!(error.to_string().contains("macro view failed"));
    both(
        "(defmacro recovered [] (vec (eduction (map inc) (take 2) (range)))) [(into [] (eduction (map inc) (take 2) (range))) (recovered)]",
        "[[1 2] [1 2]]",
    );
}

#[test]
fn cancellation_in_eduction_construction_demand_and_completion_is_terminal() {
    use std::sync::{Arc, Mutex};
    let p = program(
        r#"
      (let [xf (fn [rf] (js/eventEduction "transform")
                 (fn ([a] (js/eventEduction "completion") (rf a))
                     ([a x] (js/eventEduction "step") (rf a x))))
            s (eduction xf (lazy-seq (js/eventEduction "source") (list 1 2)))]
        (vec s))"#,
    );
    let expected = ["transform", "source", "step", "step", "completion"];
    for stage in ["transform", "source", "step", "completion"] {
        let e = Evaluator::new(EvaluatorOptions::default()).unwrap();
        let events = Arc::new(Mutex::new(Vec::new()));
        let captured = events.clone();
        e.install_callback("eventEduction", move |control, event| {
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
        let recovered = Evaluator::new(EvaluatorOptions::default()).unwrap();
        let finite = program("(into [] (eduction (map inc) (take 2) (range)))");
        assert_eq!(
            recovered
                .print(recovered.evaluate(&finite).unwrap())
                .unwrap(),
            "[1 2]"
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
        const s = c.eduction(xf, c.take(2), c.range());
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
