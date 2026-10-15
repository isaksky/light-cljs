//! Printing controls, writer options and output scopes in both canonical engines.
use crate::temp_dir::TempDir;
use light_cljs::evaluator::{Evaluator, EvaluatorOptions, RUNTIME_SOURCE};
use light_cljs::{MacroHost, analyze_with_macros, emitter::emit_js_unit, parse};
use std::process::Command;

fn both(source: &str, expected: &str) {
    if source.trim_start().starts_with("(ns cljs.printing-test)") {
        let root = TempDir::new("cljs-printing");
        std::fs::create_dir_all(root.join("cljs")).unwrap();
        std::fs::write(root.join("cljs/printing_test.cljs"), source).unwrap();
        let js = light_cljs::compile_namespace_with_macros(
            "cljs.printing-test",
            std::slice::from_ref(root.path()),
            &mut MacroHost::new(),
        )
        .unwrap();
        let e = Evaluator::new(EvaluatorOptions::default()).unwrap();
        assert_eq!(e.print(e.evaluate_js(&js).unwrap()).unwrap(), expected);
        let out = Command::new("node")
            .args(["-e", &format!("console.log({js});")])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(String::from_utf8(out.stdout).unwrap().trim(), expected);
        return;
    }
    let p = analyze_with_macros(source, &parse(source).unwrap(), &mut MacroHost::new())
        .unwrap_or_else(|e| panic!("{source}: {e}"));
    let e = Evaluator::new(EvaluatorOptions::default()).unwrap();
    assert_eq!(
        e.print(e.evaluate(&p).unwrap()).unwrap(),
        expected,
        "QuickJS: {source}"
    );
    let out = Command::new("node")
        .args([
            "-e",
            &format!(
                "const rt = {RUNTIME_SOURCE}; console.log(rt.core.prStr(({})(rt)));",
                emit_js_unit(&p)
            ),
        ])
        .output()
        .unwrap();
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
fn length_level_readability_metadata_and_option_overrides() {
    both(
        r##"
      (defrecord PrintingRecord [a b])
      (defn printing-witness []
        (assert (= "" (pr-str)))
        (assert (= "#" (binding [*print-readably* false *print-length* 1 *print-level* 0 *print-meta* true]
                           (str (with-meta ["x" [1 2]] {:a true})))))
        (assert (= "\n" (prn-str)))
        (assert (= "-0.0 ##NaN ##Inf ##-Inf" (pr-str -0.0 js/NaN js/Infinity (- js/Infinity))))
        (assert (= "\"escape chars \\t \\r \\n \\\\ \\\" \\b \\f\"" (pr-str "escape chars \t \r \n \\ \" \b \f")))
        (assert (= "[\"üñîçó∂£\" :ทดสอบ/你好 こんにちは]" (pr-str ["üñîçó∂£" :ทดสอบ/你好 'こんにちは])))
        (assert (= "[1 ...] {...} #{...} #js [1 ...] #js {...}"
                   (str (binding [*print-length* 1] (pr-str [1 2])) " "
                        (binding [*print-length* 0] (pr-str {:a 1 :b 2} #{1 2})) " "
                        (binding [*print-length* 1] (pr-str #js [1 2])) " "
                        (binding [*print-length* 0] (pr-str #js {:a 1})))))
        (assert (= "[[1 #] {:a #}]" (binding [*print-level* 2] (pr-str [[1 [2]] {:a [3]}]))))
        (assert (= "# # # # #js #" (binding [*print-level* 0] (pr-str [1] {:a 1} #{1} #js [1] #js {:a 1}))))
        (assert (= "[1 2]" (binding [*print-length* 0] (pr-str-with-opts [[1 2]] {:readably true}))))
        (assert (= "[1 …] {:a 1, …} #{1 …} #js [1 …]"
                   (pr-str-with-opts [[1 2] (array-map :a 1 :b 2) (sorted-set 1 2) #js [1 2]]
                                     {:readably true :print-length 1 :more-marker "…"})))
        (assert (= "(1 …)" (pr-str-with-opts [(eduction (map identity) [1 2])] {:print-length 1 :more-marker "…"})))
        (assert (= "#cljs.user.PrintingRecord{:a ..., ...}" (binding [*print-length* 1] (pr-str (->PrintingRecord 1 2)))))
        (assert (= "#cljs.user.PrintingRecord{#, #}" (binding [*print-level* 1] (pr-str (->PrintingRecord [1] 2)))))
        (assert (= "#cljs.user.PrintingRecord{:a #, :b 2}" (binding [*print-level* 2] (pr-str (->PrintingRecord [1] 2)))))
        (assert (= "#" (binding [*print-level* 0] (pr-str (->PrintingRecord [1] 2)))))
        (assert (= "#:sample{:a 1, :b 2}" (binding [*print-namespace-maps* true] (pr-str {:sample/a 1 :sample/b 2}))))
        (assert (= "^{:tag \"v\"} [\"x\"]" (binding [*print-meta* true] (pr-str (with-meta ["x"] {:tag "v"})))))
        (assert (= "[x]" (binding [*print-meta* true *print-readably* true]
                              (pr-str-with-opts [(with-meta ["x"] {:tag "v"})] {}))))
        (assert (= "x [y]" (print-str "x" ["y"])))
        (assert (= "x\n" (println-str "x")))
        (assert (= "\"x\"\n" (prn-str "x")))
        (assert (nil? (flush)))
        true)
      (defmacro printing-macro-witness [] (printing-witness))
      [(printing-witness) (printing-macro-witness)]"##,
        "[true true]",
    );
}

#[test]
fn javascript_object_prefixes_survive_depth_limits_in_both_phases_and_sessions() {
    const SOURCE: &str = r##"
      (def prefix-calls (atom 0))
      (defn prefix-observations []
        (swap! prefix-calls inc)
        (let [settings [*print-level* *print-length* *print-readably* *print-meta*]
              observations
              [(binding [*print-level* 0]
                 (pr-str [1] {:a 1} #{1} #js [1] #js {:a 1}))
               (binding [*print-level* 1]
                 (pr-str [#js {:a 1} #js [1] {:a 1} [1] #{1}]))
               (binding [*print-level* 2]
                 (pr-str [#js {:a #js {:b 1} :b #js [1]}]))
               (binding [*print-level* 1 *print-length* 1 *print-readably* false]
                 (pr-str #js {:a #js {:b "x"} :b "y"}))
               (binding [*print-level* 0]
                 (pr-str-with-opts [#js {:a 1} #js [1]] {:readably true :print-length 0}))
               (binding [*print-meta* true *print-level* 1]
                 (pr-str (with-meta [#js {:a 1}] {:tag "v"})))]]
          (assert (= "printer" (try
            (binding [*print-level* 1 *print-length* 0 *print-readably* false *print-meta* true]
              (pr-str-with-opts [#js {:a #js {:b 1}}]
                {:alt-impl (fn [x writer opts]
                  (if (keyword? x) (throw (js/Error. "printer"))
                    ((:fallback-impl opts) x writer opts)))}))
            (catch js/Error e (.-message e)))))
          (conj observations
            (= settings [*print-level* *print-length* *print-readably* *print-meta*])
            (pr-str #js {:a 1} #js [1]))))
      (defmacro prefix-macro [] (prefix-observations))
      [(prefix-macro) @prefix-calls (prefix-observations) @prefix-calls]
    "##;
    const OBSERVATIONS: &str = r##"["# # # # #js #" "[#js # # # # #]" "[#js {:a #js #, :b #}]" "#js {:a #js #, ...}" "#js # #" "^{:tag \"v\"} [#js #]" true "#js {:a 1} #js [1]"]"##;
    let expected = format!("[{OBSERVATIONS} 0 {OBSERVATIONS} 1]");
    both(SOURCE, &expected);

    let mut session = light_cljs::EvalSession::new(light_cljs::EvalOptions::default()).unwrap();
    for iteration in 0..2 {
        let source = if iteration == 0 {
            SOURCE
        } else {
            "[(prefix-macro) @prefix-calls (prefix-observations) @prefix-calls]"
        };
        let result = session.eval_string(source).unwrap().value;
        assert_eq!(
            session.print(result).unwrap(),
            format!(
                "[{OBSERVATIONS} {iteration} {OBSERVATIONS} {}]",
                iteration + 1
            )
        );
        session.release(result).unwrap();
        let settings = session
            .eval_string("[*print-level* *print-length* *print-readably* *print-meta*]")
            .unwrap()
            .value;
        assert_eq!(session.print(settings).unwrap(), "[nil nil true false]");
        session.release(settings).unwrap();
    }
    let mut fresh = session.fresh_session().unwrap();
    let result = fresh.eval_string(SOURCE).unwrap().value;
    assert_eq!(fresh.print(result).unwrap(), expected);
    fresh.release(result).unwrap();
}

#[test]
fn alternative_printers_receive_fallback_and_recurse_with_options() {
    both(
        r##"
      (defn alternative-witness [] (let [opts {:readably true :alt-impl
                  (fn [obj writer opts]
                    (if (keyword? obj)
                      (-write writer (str "|" (name obj) "|"))
                      ((:fallback-impl opts) obj writer opts)))}
            x (reify IPrintWithWriter
                 (-pr-writer [_ writer opts]
                   (-write writer (str "custom:" (:label opts)))))]
        (assert (= "[|start| 1 [|middle|] |end|] |standalone|"
                   (pr-str-with-opts [[:start 1 [:middle] :end] :standalone] opts)))
        (assert (= "" (pr-str-with-opts [[1 2]] {:alt-impl (fn [_ _ _] nil)})))
        (assert (= "custom:ok" (pr-str-with-opts [x] {:label "ok"})))
        (assert (= "[<1> <2>]" (pr-str-with-opts [[1 2]]
                   {:alt-impl (fn [x writer opts]
                     (if (number? x) (-write writer (str "<" x ">"))
                       ((:fallback-impl opts) x writer opts)))})))
        (assert (= "custom:\n" (prn-str-with-opts [x] nil)))
        (assert (= "1" (binding [*print-dup* true] (pr-str 1))))
        true))
      (defmacro alternative-macro-witness [] (alternative-witness))
      [(alternative-witness) (alternative-macro-witness)]"##,
        "[true true]",
    );
}

#[test]
fn output_sinks_capture_nesting_flush_and_restoration() {
    both(
        r##"
      (defn output-witness [] (let [events (atom []) sink #(swap! events conj [:out %]) err #(swap! events conj [:err %])]
        (binding [*print-fn* sink *print-err-fn* err]
          (assert (identical? sink (set-print-fn! sink)))
          (assert (identical? err (set-print-err-fn! err)))
          (print "a" "b") (pr "c") (*print-err-fn* "error") (string-print "direct")
          (assert (= [[:out "a b"] [:out "\"c\""] [:err "error"] [:out "direct"]] @events))
          (reset! events [])
          (binding [*print-newline* false] (println "quiet") (prn "quiet"))
          (assert (= [[:out "quiet"] [:out "\"quiet\""]] @events))
          (assert (= "outerinner\nend" (with-out-str
                     (print "outer") (print (with-out-str (println "inner"))) (print "end"))))
          (assert (= "yes\n" (binding [*print-newline* false] (with-out-str (println "yes")))))
          (try (with-out-str (print "hidden") (throw (js/Error. "capture"))) (catch js/Error _ nil))
          (assert (identical? sink *print-fn*))
          (assert (true? *print-newline*))
          (binding [*print-fn* (fn [_] (throw (js/Error. "sink"))) *print-readably* true]
            (try (print "oops") (catch js/Error _ nil))
            (assert (true? *print-readably*)))
          (reset! events [])
          (with-redefs [flush #(swap! events conj :flush)]
            (newline) (newline {:flush-on-newline false})
            (binding [*flush-on-newline* false] (println "line"))
            (prn :done))
          (assert (= [[:out "\n"] :flush [:out "\n"] [:out "line"] [:out "\n"]
                      [:out ":done"] [:out "\n"] :flush] @events)))
        true))
      (defmacro output-macro-witness [] (output-witness))
      [(output-witness) (output-macro-witness)]"##,
        "[true true]",
    );
}

#[test]
fn utf16_strings_preserve_code_units_in_printing_and_captured_output() {
    both(
        r##"
      (let [s (.fromCharCode js/String 0 31 55296 56320 55296)
            readable (pr-str s) captured (with-out-str (print s))]
        (assert (= 7 (count readable)))
        (assert (= [34 0 31 55296 56320 55296 34]
                   (mapv #(.charCodeAt readable %) (range (count readable)))))
        (assert (= s captured (print-str s)))
        (assert (= "[0,31,55296,56320,55296]"
                   (.stringify js/JSON (to-array (mapv #(.charCodeAt captured %) (range (count captured)))))))
        true)"##,
        "true",
    );
}

#[test]
fn configured_host_output_and_error_sinks_remain_independent_after_failure() {
    use light_cljs::{EvalOptions, EvalSession};
    use std::{cell::RefCell, rc::Rc};
    let out = Rc::new(RefCell::new(Vec::<String>::new()));
    let err = Rc::new(RefCell::new(Vec::<String>::new()));
    let out_sink = out.clone();
    let err_sink = err.clone();
    let mut s = EvalSession::new(EvalOptions {
        output: Some(Rc::new(move |text| {
            out_sink.borrow_mut().push(text.to_owned());
            if text == "fail" {
                return Err(light_cljs::evaluator::EvalError("output failed".into()));
            }
            Ok(())
        })),
        error_output: Some(Rc::new(move |text| {
            err_sink.borrow_mut().push(text.to_owned());
            Ok(())
        })),
        ..Default::default()
    })
    .unwrap();
    s.eval_string("(println \"😀λ\") (*print-err-fn* \"error λ\")")
        .unwrap();
    assert!(
        s.eval_string("(print \"fail\")")
            .unwrap_err()
            .to_string()
            .contains("output failed")
    );
    s.eval_string("(assert (true? *print-readably*)) (println \"recovered\")")
        .unwrap();
    assert_eq!(*out.borrow(), ["😀λ", "\n", "fail", "recovered", "\n"]);
    assert_eq!(*err.borrow(), ["error λ"]);
}

#[test]
fn complete_upstream_printing_bodies_execute_in_applications_and_macros() {
    let fixtures = include_str!("../fixtures/compiler-printing-completion.tsv");
    let rows: Vec<_> = fixtures
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .collect();
    assert_eq!(rows.len(), 3);
    for row in rows {
        let columns: Vec<_> = row.split('\t').collect();
        assert_eq!(columns.len(), 4);
        both(columns[1], columns[2]);
        let body_start = columns[1].find("(let [utility-failures").unwrap();
        let (setup, body) = columns[1].split_at(body_start);
        both(
            &format!("{setup} (defmacro printing-body-witness [] {body}) (printing-body-witness)"),
            columns[2],
        );
    }
}

#[test]
fn public_writer_helpers_and_console_hooks_execute_in_both_phases() {
    both(
        r##"
      (defn helper-witness []
        (let [events (atom []) writer (reify IWriter
               (-write [_ text] (swap! events conj text) nil)
               (-flush [_] (swap! events conj :flushed) nil))]
          (pr-sequential-writer writer (fn [x w opts] (-write w (str x)))
                                "<" ";" ">" {:print-length 1 :more-marker "etc"} [1 2])
          (assert (= ["<" "1" ";" "etc" ">"] @events))
          (reset! events [])
          (pr-seq-writer [1 "x"] writer {:readably true}) (-flush writer)
          (assert (= ["1" " " "\"x\"" :flushed] @events))
          (reset! events [])
          (-pr-writer (with-meta [(with-meta [1] {:child true})] {:root true})
                      writer {:readably true :meta true})
          (assert (= "[^{:child true} [1]]" (apply str @events)))
          (assert (true? (print-meta? {:meta true} (with-meta [] {}))))
          (assert (false? (print-meta? {} (with-meta [] {}))))
          (assert (false? (print-meta? {:meta true} nil)))
          (let [old (.-console js/globalThis)
                console #js {:log (fn [& xs] (swap! events conj [:log (vec xs)]))
                             :error (fn [& xs] (swap! events conj [:error (vec xs)]))}]
            (binding [*print-fn* nil *print-err-fn* nil *print-newline* true]
              (try
                (set! (.-console js/globalThis) console)
                (assert (nil? (enable-console-print!)))
                (assert (false? *print-newline*))
                (reset! events [])
                (println "line") (*print-err-fn* "err")
                (assert (= [[:log ["line"]] [:error ["err"]]] @events))
                (finally (set! (.-console js/globalThis) old))))))
        true)
      (defmacro helper-macro-witness [] (helper-witness))
      [(helper-witness) (helper-macro-witness)]"##,
        "[true true]",
    );
}

#[test]
fn rust_display_escapes_isolated_surrogates_and_preserves_valid_pairs() {
    let e = Evaluator::new(EvaluatorOptions::default()).unwrap();
    let value = e
        .evaluate_js("String.fromCharCode(0xd800, 0x61, 0xdc00, 0xd83d, 0xde00)")
        .unwrap();
    assert_eq!(e.print(value).unwrap(), "\"\\ud800a\\udc00😀\"");
    let units = e.evaluate_js(&format!("JSON.stringify({RUNTIME_SOURCE}.core.prStr(String.fromCharCode(0xd800, 0x61, 0xdc00, 0xd83d, 0xde00)))")).unwrap();
    assert_eq!(e.string(units).unwrap(), "\"\\\"\\ud800a\\udc00😀\\\"\"");
}

/// Complete sources in both compiler phases and both host lifecycles, each run in
/// QuickJS and Node (see `tests/support/phase_oracle.rs`).
mod phase_oracles {
    use crate::phase_oracle::{Case, run};

    #[test]
    fn printing_test_namespaces() {
        run(&[
            Case::namespace(
                "test-print-with-opts",
                "cljs.printing-test",
                "(run nil)",
                include_str!(
                    "../fixtures/compiler-printing-completion/test-print-with-opts/cljs/printing_test.cljs"
                ),
            ),
            Case::namespace(
                "test-print-knobs",
                "cljs.printing-test",
                "(run nil)",
                include_str!(
                    "../fixtures/compiler-printing-completion/test-print-knobs/cljs/printing_test.cljs"
                ),
            ),
        ]);
    }

    #[test]
    fn printing_helpers() {
        run(&[
Case::expression("print-err-callback-failure", include_str!("../fixtures/phase-oracles/printing-completion/print-err-callback-failure.cljs")),
Case::expression("string-buffer-writer", include_str!("../fixtures/public-print-helper-gap/public-string-buffer-writer.cljs")).printing(r#"["a" nil "a"]"#),
Case::expression("print-writer-factories", include_str!("../fixtures/phase-oracles/printing-completion/print-writer-factories.cljs")).printing(r#"[true true [["a" true]] [["a" true] ["b" true]] [["a" true] ["b" true] ["c" true]] nil [["a" true] ["b" true] ["c" true]]]"#),
Case::expression("print-meta-unreadable", include_str!("../fixtures/phase-oracles/printing-completion/print-meta-unreadable.cljs")).printing(r#""^{:x true} [x]""#),
Case::observed("writer-flush", include_str!("../fixtures/phase-oracles/printing-completion/writer-flush.cljs"), r#"{"id":"writer-flush","outcome":{"status":"returned","value":true},"trace":[],"stdout":""}"#),
]);
    }
}
