//! Strict EDN, compiled tag handlers and explicit synchronous reader capabilities.
use light_cljs::{
    MacroHost, analyze_with_macros,
    evaluator::{EvalError, Evaluator, EvaluatorOptions},
    parse,
};
use std::{cell::RefCell, process::Command, rc::Rc};
#[path = "support/sci_protocol.rs"]
#[allow(dead_code)]
mod protocol;

fn program(source: &str) -> light_cljs::Program {
    let mut host = MacroHost::new();
    host.disable_native_evaluation();
    analyze_with_macros(source, &parse(source).unwrap(), &mut host)
        .unwrap_or_else(|e| panic!("{source}: {e}"))
}
fn both(source: &str, expected: &str) {
    use std::{
        fs,
        io::Write,
        process::Stdio,
        sync::atomic::{AtomicUsize, Ordering},
    };
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "light-edn-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    let syntax = parse(source).unwrap();
    let last = syntax.nodes[syntax.roots[syntax.roots.len() - 1].0]
        .span
        .clone();
    let source = format!(
        "(ns main) {} (pr-str {})",
        &source[..last.start],
        &source[last]
    );
    fs::write(dir.join("main.cljs"), &source).unwrap();
    let mut host = MacroHost::new();
    host.disable_native_evaluation();
    let js =
        light_cljs::compile_namespace_with_macros("main", std::slice::from_ref(&dir), &mut host)
            .unwrap_or_else(|e| panic!("{source}: {e}"));
    fs::remove_dir_all(&dir).unwrap();
    let e = Evaluator::new(EvaluatorOptions {
        timeout: std::time::Duration::from_secs(60),
        ..EvaluatorOptions::default()
    })
    .unwrap();
    assert_eq!(
        e.string(
            e.evaluate_js(&format!(
                "(() => {{ globalThis.goog = {{object: {{get: (o,k) => o[k]}}}}; return {js}; }})()"
            ))
            .unwrap()
        )
        .unwrap(),
        expected,
        "QuickJS: {source}"
    );
    let mut child = Command::new("node")
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(
            format!("globalThis.goog = {{object: {{get: (o,k) => o[k]}}}}; console.log({js});")
                .as_bytes(),
        )
        .unwrap();
    let out = child.wait_with_output().unwrap();
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

fn quoted(units: &[u16]) -> String {
    let mut result = String::from("\"");
    for unit in units {
        result.push_str(&format!("\\u{unit:04x}"));
    }
    result.push('"');
    result
}
fn literal(value: &protocol::Value) -> String {
    use protocol::Value as V;
    match value {
        V::Nil => "nil".into(),
        V::Bool(x) => x.to_string(),
        V::Int(x) => x.to_string(),
        V::Float(bits) => {
            let x = f64::from_bits(*bits);
            if x.is_nan() {
                ":nan".into()
            } else if x == f64::INFINITY {
                "##Inf".into()
            } else if x == f64::NEG_INFINITY {
                "##-Inf".into()
            } else {
                format!("{x:?}")
            }
        }
        V::Ratio(n, d) => format!("(/ {n} {d})"),
        V::Char(unit) => quoted(&[*unit]),
        V::String(units) => quoted(units),
        V::Symbol(ns, name) | V::Keyword(ns, name) => format!(
            "({} {} {})",
            if matches!(value, V::Symbol(..)) {
                "symbol"
            } else {
                "keyword"
            },
            ns.as_deref().map(quoted).unwrap_or_else(|| "nil".into()),
            quoted(name)
        ),
        V::Vector(values) | V::List(values) | V::Seq(values) | V::Set(values) => format!(
            "({} {})",
            match value {
                V::Vector(..) => "vector",
                V::Set(..) => "hash-set",
                _ => "list",
            },
            values.iter().map(literal).collect::<Vec<_>>().join(" ")
        ),
        V::Map(pairs) => format!(
            "(hash-map {})",
            pairs
                .iter()
                .flat_map(|(k, v)| [literal(k), literal(v)])
                .collect::<Vec<_>>()
                .join(" ")
        ),
        _ => panic!("Outside retained EDN profile"),
    }
}

#[test]
fn migrated_native_edn_values_effects_and_errors_execute_without_interpreter() {
    let cases = protocol::cases(include_str!("fixtures/sci-outcomes.tsv")).unwrap();
    let mut count = 0;
    for case in cases
        .iter()
        .filter(|c| c.id.starts_with("sci-data-") && c.source.contains("clojure.edn"))
    {
        let mut fixture_source = case.source.replace("(catch Exception ", "(catch js/Error ");
        let mut prelude = String::new();
        if fixture_source.starts_with("(require ") {
            let syntax = parse(&fixture_source).unwrap();
            let span = syntax.nodes[syntax.roots[0].0].span.clone();
            prelude = fixture_source[span.clone()].to_owned();
            fixture_source = fixture_source[span.end..].to_owned();
        }
        let actual = match &case.outcome {
            protocol::Outcome::Returned(expected) => format!(
                "(let [actual (do {}) normalized (w/postwalk (fn [x] (if (and (number? x) (js* \"Number.isNaN(~{{x}})\")) :nan x)) actual)] (= normalized {}))",
                fixture_source,
                literal(expected)
            ),
            protocol::Outcome::Error { .. } => {
                format!(
                    "(try (do {}) false (catch js/Error e true))",
                    fixture_source
                )
            }
        };
        let effects = literal(&protocol::Value::Vector(case.effects.clone()));
        let source = format!(
            "(require '[clojure.edn] '[clojure.walk :as w]) (def effects (atom [])) (defn observe [v] (swap! effects conj v) v) {prelude} [ {actual} (= @effects {effects}) ]"
        );
        both(&source, "[true true]");
        assert!(
            case.stdout.is_empty() && case.stderr.is_empty(),
            "{}",
            case.id
        );
        count += 1;
    }
    assert_eq!(count, 61);
}

#[test]
fn compiled_reader_registration_builtins_metadata_unicode_and_macros() {
    both(
        r##"(require '[cljs.reader :as r] '[clojure.edn :as edn])
      (r/register-tag-parser! 'plus inc)
      (defmacro from-data [] (r/register-tag-parser! 'plus inc) (r/read-string "#plus 41"))
      [(from-data) (r/read-string "#plus 4")
       (try (edn/read-string "#plus 4") false (catch js/Error e true))
       (meta (r/read-string "^:m [1]"))
       (r/read-string "#:foo{:x 1 :_/y 2 z 3}")
       (= (r/read-string "#uuid \"550e8400-e29b-41d4-a716-446655440000\"") #uuid "550e8400-e29b-41d4-a716-446655440000")
       (inst-ms (r/read-string "#inst \"2010-11-12T13:14:15.666-05:00\""))
       (r/read-string "#queue [1 2]")
       (mapv r/read-string ["052" "+0x2a" "2r101010" "8R52" "16r2a" "36r16"])
       (r/read-string "[اختبار ทดสอบ こんにちは 你好 cześć привет]")
       (fn? (r/deregister-tag-parser! 'plus))
       (let [previous (r/register-tag-parser! 'queue inc)
             result [(fn? previous) (r/read-string "#queue 4")
                     (r/read-string {:readers {'queue dec}} "#queue 4")]]
         (r/register-tag-parser! 'queue previous)
         (conj result (r/read-string "#queue [1 2]")))]"##,
        "[42 5 true {:m true} {:foo/x 1, :y 2, foo/z 3} true 1289585655666 #queue [1 2] [42 42 42 42 42 42] [اختبار ทดสอบ こんにちは 你好 cześć привет] true [true 5 3 #queue [1 2]]]",
    );
}

#[test]
fn streams_share_cursor_reentry_is_located_and_nested_inputs_restore() {
    both(
        r##"(require '[cljs.reader :as r] '[clojure.edn :as edn])
      (let [chunks (atom ["#tag\n" "[1\n" "2]\nnil\r\n42"])
            effects (atom [])
            s (r/input-provider "stream.edn" (fn [] (let [x (first @chunks)] (swap! chunks rest) x)))
            opts {:readers {'tag (fn [x] (swap! effects conj x) x)}}]
        [(edn/read opts s) (r/read-line s) (edn/read s) (r/read-line s)
         (edn/read s) (edn/read {:eof false} s) @effects
         (with-in-str "1 2" [(read) (edn/read) (with-in-str "3" (read)) (read {:eof :done} (compiler-reader-current-input))])])"##,
        "[[1 2] \"\" nil \"\" 42 false [[1 2]] [1 2 3 :done]]",
    );
    both(
        r##"(require '[cljs.reader :as r] '[clojure.edn :as edn])
      (let [s (r/string-push-back-reader "#tag 1 2" "reentry.edn")]
        (try (edn/read {:readers {'tag (fn [v] (edn/read s))}} s)
          (catch js/Error e [(.-category e) (.-source e) (.-line e) (.-column e)])))"##,
        "[\"host\" \"reentry.edn\" 1 7]",
    );
}

#[test]
fn typed_results_reject_host_objects_cycles_and_deferred_values_without_effects() {
    both(
        r##"(require '[clojure.edn :as edn])
      (let [seen (atom [])]
        [(mapv (fn [f] (try (edn/read-string {:readers {'tag f}} "#tag 1") :bad (catch js/Error e :caught)))
           [(fn [v] (atom v)) (fn [v] (fn [] v)) (fn [v] (js* "({})"))
            (fn [v] (js* "(() => { const a = []; a.push(a); return a; })()"))
            (fn [v] (lazy-seq (swap! seen conj :bad) [v]))]) @seen])"##,
        "[[:caught :caught :caught :caught :caught] []]",
    );
}

#[test]
fn rust_providers_supply_chunks_only_and_errors_remain_bounded_and_located() {
    let e = Evaluator::new(EvaluatorOptions::default()).unwrap();
    let chunks = Rc::new(RefCell::new(["[1\n", "2]\n"].into_iter()));
    let provider_chunks = chunks.clone();
    let stream = e
        .input_provider("host.edn", move |control| {
            control.poll()?;
            Ok(provider_chunks.borrow_mut().next().map(str::to_owned))
        })
        .unwrap();
    let function = e
        .evaluate(&program(
            "(fn [s] [(compiler-edn-read s) (compiler-edn-read {:eof :done} s)])",
        ))
        .unwrap();
    assert_eq!(
        e.print(e.call(function, &[stream]).unwrap()).unwrap(),
        "[[1 2] :done]"
    );
    let stream = e
        .input_provider("failed.edn", |_| {
            Err(EvalError("private provider details".into()))
        })
        .unwrap();
    let function = e.evaluate(&program("(fn [s] (try (compiler-edn-read s) (catch js/Error e [(.-category e) (.-source e) (.-line e) (.-column e) (.-message e)])))")).unwrap();
    assert_eq!(
        e.print(e.call(function, &[stream]).unwrap()).unwrap(),
        "[\"host\" \"failed.edn\" 1 1 \"Input provider failed at failed.edn:1:1\"]"
    );
}

#[test]
fn source_reader_syntax_exports_compiler_owned_macros_and_suppresses_skipped_tags() {
    both(
        r##"(require '[cljs.reader :as r])
      (defmacro anonymous [] (read-string "#(+ %1 %2)"))
      (defmacro quoted [] (read-string "`[~(+ 1 2) ~@[4 5]]"))
      (defmacro auto [] (read-string "`(let [x# 2] (+ x# x#))"))
      (let [effects (atom [])]
        (r/register-tag-parser! 'tag (fn [v] (swap! effects conj v) v))
        [((anonymous) 2 3) (quoted) (let [form (auto)] (= (first (second form)) (second (nth form 2)) (nth (nth form 2) 2)))
         (read-string "[0 #?@(:clj [#tag 99] :cljs [#tag 1 2]) 3]")
         @effects (read-string "#\"\\d+\"")
         (= (read-string "'x") '(quote x))])"##,
        "[5 [3 4 5] true [0 1 2 3] [1] #\"\\d+\" true]",
    );
}

#[test]
fn cljs_builtin_js_literals_empty_input_timestamps_and_duplicate_errors() {
    both(
        r##"(require '[cljs.reader :as r])
      [(nil? (r/read-string ""))
       (aget (r/read-string "#js {:foo #js [1 2]}" ) "foo" 1)
       (= (inst-ms (r/read-string "#inst \"1582-10-04T23:59:59.999\"")) (inst-ms #inst "1582-10-04T23:59:59.999Z"))
       (mapv (fn [s] (try (r/read-string s) false (catch js/Error e true)))
         ["{:a 1 :a 2}" "#{1 1}" "\"abc \\ua\"" "\"abc \\x0z\"" "#inst \"2019-02-29\""])]"##,
        "[true 2 true [true true true true true]]",
    );
}

#[test]
fn reader_limits_and_cancelled_providers_share_sticky_session_control() {
    let e = Evaluator::new(EvaluatorOptions::default()).unwrap();
    let value = e.evaluate_unit(r#"rt => {
      const c = rt.core;
      return ['['.repeat(70)+'0'+']'.repeat(70), ' '.repeat(1048577)].map(text => {
        try { c.ednReadString(text); return false; } catch (error) { return error.category === 'limit' && /limit/.test(error.message); }
      }).every(Boolean);
    }"#).unwrap();
    assert_eq!(e.print(value).unwrap(), "true");
    let e = Evaluator::new(EvaluatorOptions {
        poll_budget: 1000,
        ..Default::default()
    })
    .unwrap();
    let error = e.evaluate_unit(r#"rt => {
      try { rt.core.ednReadString(rt.core.hashMap(rt.keyword(null,'readers'),rt.core.hashMap(rt.symbol(null,'tag'),() => {while(true) {}})), '#tag 1'); }
      catch (_) {} return 42;
    }"#).unwrap_err();
    assert!(error.0.contains("budget"), "{error}");
    assert!(e.evaluate_js("42").is_err());
    let e = Evaluator::new(EvaluatorOptions::default()).unwrap();
    let stream = e
        .input_provider("cancel.edn", |control| {
            control.cancel();
            Ok(Some("1".into()))
        })
        .unwrap();
    let function = e
        .evaluate(&program("(fn [s] (compiler-edn-read s))"))
        .unwrap();
    assert!(
        e.call(function, &[stream])
            .unwrap_err()
            .0
            .contains("cancelled")
    );
    assert!(e.evaluate_js("42").is_err());
}

#[test]
fn migrated_native_core_stream_reader_arities_eof_values_and_data() {
    let ids = [
        "sci-io-read-line-mixed",
        "sci-io-read-default",
        "sci-io-read-arities",
        "sci-io-eof-eofthrow",
        "sci-io-eof-catch",
        "sci-io-eof-map-nil",
        "sci-io-eof-string-catch",
        "sci-io-read-first-only",
        "sci-io-read-data",
        "sci-io-read-comment",
        "sci-io-read-multi",
        "sci-io-read-eval-disabled",
        "sci-io-read-incomplete",
        "sci-io-read-invalid",
        "sci-io-read-unknown-tag",
        "sci-io-read-feature",
        "sci-io-read-string-arity",
        "sci-io-read-string-type",
    ];
    let cases = protocol::cases(include_str!("fixtures/sci-outcomes.tsv")).unwrap();
    let mut count = 0;
    for case in cases.iter().filter(|case| ids.contains(&case.id)) {
        let source = case.source.replace("(catch Exception ", "(catch js/Error ");
        let check = match &case.outcome {
            protocol::Outcome::Returned(expected) => {
                let expected = if case.id == "sci-io-read-feature" {
                    protocol::Value::Keyword(None, "no".encode_utf16().collect())
                } else {
                    expected.clone()
                };
                format!("(= (do {source}) {})", literal(&expected))
            }
            protocol::Outcome::Error { .. } => {
                format!("(try (do {source}) false (catch js/Error e true))")
            }
        };
        both(&check, "true");
        count += 1;
    }
    assert_eq!(count, ids.len());
}

#[test]
fn complete_pinned_upstream_reader_bodies_execute_in_both_engines() {
    both(
        include_str!("fixtures/compiler-reader-upstream.cljs"),
        "7487",
    );
}

#[test]
fn scoped_input_closes_escaped_handles_and_restores_outer_cursor_on_throw() {
    both(
        r##"(require '[cljs.reader :as r])
      (let [saved (atom nil)]
        (with-in-str "inner" (reset! saved *in*))
        [(try (r/read @saved) false (catch js/Error e (= "host" (.-category e))))
         (with-in-str "outside"
           (try (with-in-str "inside" (throw (js/Error. "failure"))) (catch js/Error e nil))
           (read-line))
         (with-in-str "x" (r/close *in*))
         (nil? *in*)])"##,
        "[true \"outside\" nil true]",
    );
}

#[test]
fn cljs_reader_public_read_arities_and_option_defaults_match_pinned_source() {
    both(
        r##"(require '[cljs.reader :as r])
      (let [s (r/string-push-back-reader "1 2")]
        [(r/read s) (r/read {} s) (r/read s false :done {})
         (r/read (r/string-push-back-reader ""))
         (try (r/read-string {} "") false (catch js/Error e true))
         (r/read-string nil) (r/read-string false)])"##,
        "[1 2 :done nil true nil nil]",
    );
}

#[test]
fn cljs_reader_public_timestamp_helpers_and_dynamic_parser_atoms() {
    both(
        include_str!("fixtures/compiler-reader-public-contracts.cljs"),
        "[[2012 2 29 23 59 60 123 -330] [7 1 1 0 0 0 0 0] true true [true true true true true] [5 [unknown 8] 3 8 true true true true [10 [other 7]] true true true] true true]",
    );
}

#[test]
fn reader_atom_mutations_bindings_and_absent_tags_are_observed_in_both_phases() {
    both(
        r##"(require '[cljs.reader :as r])
      (defmacro from-state []
        (binding [r/*tag-table* (atom {'tag inc})
                  r/*default-data-reader-fn* (atom (fn [tag value] value))]
          (reset! r/*tag-table* {'tag dec})
          (+ (r/read-string "#tag 42") (r/read-string "#missing 1"))))
      (let [old @r/*tag-table*]
        [(from-state)
         (try
           (reset! r/*tag-table* {'tag inc})
           [(r/read-string "#tag 4")
            (binding [r/*tag-table* (atom {'tag dec})
                      r/*default-data-reader-fn* (atom (fn [tag value] value))]
              [(r/read-string "#tag 4") (r/read-string "#missing 8")])
            (r/read-string "#tag 4")]
           (finally (reset! r/*tag-table* old)))
         (= @r/*tag-table* old)
         (try (r/read (r/string-push-back-reader "\n#inst \"2019-02-29\"" "named.edn")) false
              (catch js/Error e [(.-source e) (.-line e) (.-column e) (.-offset e) (.-category e)]))
         (try (r/read (r/string-push-back-reader (str "#" (js* "('x'.repeat(4096))") " 1")
                                                   (js* "('n'.repeat(4096))"))) false
              (catch js/Error e [(= 256 (count (.-source e))) (<= (count (.-message e)) 1400)]))
         (binding [r/*tag-table* (atom {})]
           (r/register-tag-parser! 'scoped inc)
           (= 5 (r/read-string "#scoped 4")))
         (try (r/read-string "#scoped 4") false (catch js/Error e true))
         (binding [r/*tag-table* (atom {})]
           (mapv (fn [s] (try (r/read-string s) false (catch js/Error e true)))
                 ["#queue [1]" "#js [1]" "#uuid \"550e8400-e29b-41d4-a716-446655440000\"" "#inst \"2010\""]))])"##,
        "[42 [5 [3 8] 5] true [\"named.edn\" 2 19 19 \"reader\"] [true true] true true [true true true true]]",
    );
}
