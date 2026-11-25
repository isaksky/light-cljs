//! Canonical library values run in both engines; macros demand the same sources.
use crate::temp_dir::TempDir;
use light_cljs::compile_namespace;
#[cfg(light_v8)]
use light_cljs::evaluator::{Evaluator, EvaluatorOptions};
use std::{fs, path::PathBuf, process::Command};
struct Sources(TempDir);
impl Sources {
    fn new() -> Self {
        let path = TempDir::new("library");
        Self(path)
    }
    fn write(&self, path: &str, source: &str) {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, source).unwrap();
    }
    fn compile(&self, body: &str) -> String {
        self.write(
            "probe/main.cljs",
            &format!("(ns probe.main (:require [clojure.set :as s] [clojure.walk :as w])) {body}"),
        );
        compile_namespace("probe.main", std::slice::from_ref(self.0.path())).unwrap()
    }
}
fn both(body: &str, expected: &str) {
    let sources = Sources::new();
    let js = sources.compile(body);
    #[cfg(light_v8)]
    {
        let evaluator = Evaluator::new(Default::default()).unwrap();
        let value = evaluator
            .evaluate_js(&format!("JSON.stringify({js})"))
            .unwrap();
        assert_eq!(
            evaluator.string(value).unwrap(),
            expected,
            "embedded engine {body}"
        );
    }
    assert_eq!(
        crate::canonical_json::node_stdout(&format!("console.log(JSON.stringify({js}));")).trim(),
        expected,
        "Node {body}"
    );
}
#[test]
fn persistent_set_algebra_and_relations() {
    both(
        r#"(let [x (with-meta #{1 2} {:m :x}) y (with-meta #{2 3 4} {:m :y})]
      #js [(= (s/union x y) #{1 2 3 4}) (= (s/intersection x y) #{2})
      (= (s/difference x y #{8}) #{1}) (= (meta (s/union x y)) {:m :y})
      (= (meta (s/intersection x y)) {:m :x}) (s/subset? #{2} y) (s/superset? y #{3})
      (= (s/select odd? y) #{3}) (= (s/rename-keys {:a 1 :b 2} {:a :b :b :a}) {:a 2 :b 1})
      (= (s/project #{{:a 1 :b 2} {:a 1 :b 3}} [:a]) #{{:a 1}})
      (= (s/index #{{:a 1 :b 2} {:a 1 :b 3}} [:a]) {{:a 1} #{{:a 1 :b 2} {:a 1 :b 3}}})
      (= (s/join #{{:a 1 :x 2} {:a 2}} #{{:a 1 :y 3}}) #{{:a 1 :x 2 :y 3}})
      (= (s/join #{{:a 1 :x 2}} #{{:b 1 :y 3}} {:a :b}) #{{:a 1 :b 1 :x 2 :y 3}})
      (= (s/rename #{{:a 1}} {:a :b}) #{{:b 1}}) (= (s/map-invert {:a 1}) {1 :a})
      (= (:arglists (meta #'s/union)) '([] [s1] [s1 s2] [s1 s2 & sets]))
      (= (:arglists (meta #'w/walk)) '([inner outer form]))])"#,
        "[true,true,true,true,true,true,true,true,true,true,true,true,true,true,true,true,true]",
    );
}
#[test]
fn walks_preserve_types_metadata_records_order_and_opaque_values() {
    both(
        r#"(defrecord R [x])
    (let [effects (atom []) observe (fn [x] (swap! effects conj (pr-str x)) x)
          original (with-meta (list 1 [2]) {:m true}) walked (w/postwalk observe original)
          post @effects _ (reset! effects []) pre (w/prewalk observe original)
          r (with-meta (assoc (->R 1) :extra [2]) {:r true})
          rr (w/postwalk (fn [x] (if (number? x) (inc x) x)) r)
          a (atom 1) d (delay (throw (js/Error. "forced")))]
      #js [(= walked original) (list? walked) (= (meta walked) {:m true})
      (= post ["1" "2" "[2]" "(1 [2])"])
      (= @effects ["(1 [2])" "1" "[2]" "2"])
      (record? rr) (= (:x rr) 2) (= (:extra rr) [3]) (= (meta rr) {:r true})
      (= (w/postwalk-replace {[1 2] :hit} [1 2]) :hit)
      (= (w/prewalk-replace {1 [2]} [1]) [[2]])
      (= (w/keywordize-keys {"a" [{"b" 1}]}) {:a [{:b 1}]})
      (= (w/stringify-keys {:ns/a {:b 1}}) {"a" {"b" 1}})
      (identical? a (w/postwalk identity a)) (identical? d (w/prewalk identity d))
      (not (realized? d))
      (map-entry? (w/postwalk identity (first {:a 1})))])"#,
        "[true,true,true,true,true,true,true,true,true,true,true,true,true,true,true,true,true]",
    );
}
#[test]
fn library_type_arity_and_ordered_failure_boundaries() {
    both(
        r#"(let [caught (fn [f] (try (f) false (catch js/Error e true))) log (atom [])]
      #js [(caught (fn [] (s/union [1]))) (caught (fn [] (s/intersection 1 #{})))
      (caught (fn [] (s/difference #{} []))) (caught (fn [] (s/select identity [])))
      (caught (fn [] (s/rename-keys 1 {}))) (caught (fn [] (s/index [1] [])))
      (caught (fn [] (s/union #{1} #{2} [3])))
      (caught (fn [] (w/postwalk-replace [] [1]))) (caught (fn [] (apply s/intersection [])))
      (caught (fn [] (w/walk (fn [entry] (swap! log conj entry) 1) identity {:a 1 :b 2})))
      (= (count @log) 1)])"#,
        "[true,true,true,true,true,true,true,true,true,true,true]",
    );
}
#[test]
fn macros_use_libraries_without_native_registration() {
    let sources = Sources::new();
    sources.write(
        "helper/lib.cljs",
        r#"(ns helper.lib (:require [clojure.set :as s] [clojure.walk :as w]))
      (defn helper [form] (w/postwalk-replace {1 42} (vec (s/intersection (set form) #{1}))))
      (defmacro compute [] (list 'quote (helper [1 2])))"#,
    );
    sources.write(
        "probe/main.cljs",
        "(ns probe.main (:require [helper.lib :refer [compute]])) (= (compute) [42])",
    );
    let js = compile_namespace("probe.main", std::slice::from_ref(sources.0.path())).unwrap();
    assert_eq!(
        crate::canonical_json::node_stdout(&format!("console.log(JSON.stringify({js}));")),
        "true\n",
        "Node"
    );
    #[cfg(light_v8)]
    {
        let e = Evaluator::new(Default::default()).unwrap();
        assert_eq!(
            e.string(e.evaluate_js(&format!("JSON.stringify({js})")).unwrap())
                .unwrap(),
            "true"
        );
    }
}
#[test]
fn traversal_limits_and_session_recovery() {
    both(
        r#"(let [bounded (fn [f] (try (f) false
                    (catch js/Error e (= (.-message e) "walk depth limit exceeded"))))]
          (letfn [(shallow [x] (w/walk shallow identity [x]))
                  (outer [x] (w/walk identity outer x))
                  (pre [x] (w/prewalk pre x))
                  (post [x] (w/postwalk post x))
                  (finite [x] (if (zero? x) x
                                (w/walk identity (fn [y] (finite (dec y))) x)))]
            #js [(bounded (fn [] (shallow 1)))
                 (= (w/walk identity identity [1]) [1])
                 (bounded (fn [] (outer 1)))
                 (= (w/walk identity identity 1) 1)
                 (bounded (fn [] (pre 1)))
                 (= (w/prewalk identity [1]) [1])
                 (bounded (fn [] (post 1)))
                 (= (w/postwalk identity [1]) [1])
                 (= (finite 4) 0)]))"#,
        "[true,true,true,true,true,true,true,true,true]",
    );
    // Exercise the embedded evaluator's limits and recovery.
    #[cfg(light_v8)]
    {
        let sources = Sources::new();
        let bad =
            sources.compile("(try (w/prewalk (fn [x] [x]) 1) (catch js/Error e (.-message e)))");
        let e = Evaluator::new(EvaluatorOptions::default()).unwrap();
        assert!(
            e.string(e.evaluate_js(&bad).unwrap())
                .unwrap()
                .contains("walk depth limit")
        );
        assert_eq!(e.number(e.evaluate_js("40+2").unwrap()).unwrap(), 42.0);
        let infinite = sources.compile("(w/postwalk identity (range))");
        let e = Evaluator::new(EvaluatorOptions {
            poll_budget: 1000,
            ..Default::default()
        })
        .unwrap();
        assert!(e.evaluate_js(&infinite).is_err());
    }
}

use crate::sci_protocol as native_fixture;
fn literal(value: &native_fixture::Value) -> String {
    use native_fixture::Value as V;
    let string = |units: &[u16]| {
        format!(
            "\"{}\"",
            units
                .iter()
                .map(|u| format!("\\u{u:04x}"))
                .collect::<String>()
        )
    };
    let values = |xs: &[V]| xs.iter().map(literal).collect::<Vec<_>>().join(" ");
    match value {
        V::Nil => "nil".into(),
        V::Bool(x) => x.to_string(),
        V::Int(x) => x.to_string(),
        V::String(x) => string(x),
        V::Char(x) => string(&[*x]),
        V::Symbol(ns, x) | V::Keyword(ns, x) => format!(
            "({} {} {})",
            if matches!(value, V::Symbol(..)) {
                "symbol"
            } else {
                "keyword"
            },
            ns.as_deref().map_or("nil".into(), string),
            string(x)
        ),
        V::List(xs) => format!("(list {})", values(xs)),
        V::Seq(xs) => format!("(seq [{}])", values(xs)),
        V::Vector(xs) => format!("[{}]", values(xs)),
        V::Set(xs) => format!("(hash-set {})", values(xs)),
        V::Map(xs) => format!(
            "(hash-map {})",
            xs.iter()
                .map(|(k, v)| format!("{} {}", literal(k), literal(v)))
                .collect::<Vec<_>>()
                .join(" ")
        ),
        _ => panic!("unexpected exact-number native fixture: {value:?}"),
    }
}
#[test]
fn all_supported_native_set_and_walk_fixtures_migrate_with_values_and_effects() {
    let cases = native_fixture::cases(include_str!("../fixtures/sci-outcomes.tsv")).unwrap();
    let sources = Sources::new();
    let mut body =
        String::from("(def effects (atom [])) (defn observe [x] (swap! effects conj x) x) #js [");
    let mut ids = Vec::new();
    for case in cases.iter().filter(|c| {
        c.id.starts_with("sci-data-")
            && (c.source.contains("clojure.set") || c.source.contains("clojure.walk"))
    }) {
        assert!(
            case.stdout.is_empty() && case.stderr.is_empty(),
            "{}",
            case.id
        );
        let source = match case.id {
            "sci-data-set-require" => "[(s/union #{1} #{2}) (union #{2} #{3})]",
            "sci-data-walk-require" => "[(w/prewalk identity [1]) (postwalk inc 2)]",
            _ => case.source,
        }
        .replace("catch Exception ", "catch js/Error ");
        // Keep the original SCI rows/expectations intact. These two historical
        // native contracts disagree with both canonical CLJS pins; this
        // migration check adopts their separately verified canonical literals.
        // Original SCI agreement remains unresolved in the library receipts.
        let result = match case.id {
            "sci-data-set-project-meta" => format!("(= (do {source}) nil)"),
            "sci-data-set-project-key-representative" => {
                format!("(= (do {source}) {{:new true}})")
            }
            _ => match &case.outcome {
                native_fixture::Outcome::Returned(expected) => {
                    format!("(= (do {source}) {})", literal(expected))
                }
                native_fixture::Outcome::Error { .. } => {
                    format!("(try (do {source}) false (catch js/Error e true))")
                }
            },
        };
        body.push_str(&format!(
            "(do (reset! effects []) (and {result} (= @effects [{}])))\n",
            case.effects
                .iter()
                .map(literal)
                .collect::<Vec<_>>()
                .join(" ")
        ));
        ids.push(case.id);
    }
    body.push(']');
    sources.write("probe/main.cljs", &format!("(ns probe.main (:require [clojure.set :as s :refer [union]] [clojure.walk :as w :refer [postwalk]])) {body}"));
    let js = compile_namespace("probe.main", std::slice::from_ref(sources.0.path())).unwrap();
    #[cfg(light_v8)]
    let observations = {
        let e = Evaluator::new(Default::default()).unwrap();
        e.string(e.evaluate_js(&format!("JSON.stringify({js})")).unwrap())
            .unwrap()
    };
    #[cfg(not(light_v8))]
    let observations =
        crate::canonical_json::node_stdout(&format!("console.log(JSON.stringify({js}));"))
            .trim()
            .to_owned();
    let parts = observations
        .trim_matches(['[', ']'])
        .split(',')
        .collect::<Vec<_>>();
    assert_eq!(parts.len(), ids.len());
    let failures = ids
        .iter()
        .zip(parts)
        .filter(|(_, passed)| *passed != "true")
        .map(|(id, _)| *id)
        .collect::<Vec<_>>();
    assert!(
        failures.is_empty(),
        "native migration differences: {failures:?}"
    );
    let mut child = Command::new("node")
        .arg("-")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::Write;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(format!("console.log(JSON.stringify({js}));").as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8(out.stdout).unwrap().trim(), observations);
    assert_eq!(ids.len(), 117);
}

#[cfg(light_v8)]
#[test]
fn registering_readers_preserves_the_serial_macro_memory_limit() {
    use light_cljs::{MacroHost, compile_with_macros, compiler_data::Name};

    let mut host = MacroHost::new();
    host.register_compiler_data_reader(Name::new(None, "named"), "(fn [x] (inc x))")
        .unwrap();
    let js = compile_with_macros(
        "(defmacro allocate [] (.-byteLength (js/ArrayBuffer. (* 40 1024 1024))))
         (str (allocate))",
        &mut host,
    )
    .unwrap();
    assert_eq!(crate::canonical_json::node(&js), "41943040");

    host.evaluator_options.memory_limit = 24 << 20;
    let js = compile_with_macros(
        "(defmacro answer [] 42) (pr-str [(answer) #named 7])",
        &mut host,
    )
    .unwrap();
    assert_eq!(crate::canonical_json::node(&js), "[42 8]");
}

#[cfg(light_v8)]
#[test]
fn compiler_data_readers_run_without_quickjs() {
    use light_cljs::{
        EvalOptions, EvalSession, MacroHost, compile_with_macros,
        compiler::{EntryRecorders, analyze_namespace_entry_recorded},
        compiler_data::Name,
        scheduler::Slots,
    };

    // The guard is process-wide, so set it in a separate test process.
    const GUARD: &str = "LIGHT_CLJS_ASSERT_NO_QUICKJS";
    if std::env::var_os(GUARD).is_none() {
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "data_libraries::compiler_data_readers_run_without_quickjs",
                "--nocapture",
            ])
            .env(GUARD, "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }

    let tag = Name::new(None, "named");
    let named = "(fn [x] (inc x))";
    let default = "(fn [tag x] [(str tag) x])";
    let source = "(when true [#named 7 #demo/missing 9])";
    let mut host = MacroHost::new();
    host.register_compiler_data_reader(tag.clone(), named)
        .unwrap();
    host.register_compiler_default_data_reader(default).unwrap();
    let js = compile_with_macros(&format!("(pr-str {source})"), &mut host).unwrap();
    let expected = r#"[8 ["demo/missing" 9]]"#;
    assert_eq!(crate::canonical_json::node(&js), expected);

    let sources = Sources::new();
    sources.write("probe/a.cljs", "(ns probe.a) (def value #named 7)");
    sources.write("probe/b.cljs", "(ns probe.b) (def value #demo/missing 9)");
    sources.write(
        "probe/main.cljs",
        "(ns probe.main (:require [probe.a :as a] [probe.b :as b]))
         (pr-str (when true [a/value b/value]))",
    );
    host.evaluator_options.memory_limit = 192 << 20;
    let (program, _) = analyze_namespace_entry_recorded(
        "probe.main",
        std::slice::from_ref(sources.0.path()),
        &mut host,
        Default::default(),
        &mut Vec::new(),
        EntryRecorders {
            slots: Slots::new(3).unwrap(),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        crate::canonical_json::node(&light_cljs::emit_js(&program)),
        expected
    );

    let mut session = EvalSession::new(EvalOptions {
        data_readers: vec![(tag, named.into())],
        default_data_reader: Some(default.into()),
        ..Default::default()
    })
    .unwrap();
    let result = session.eval_string(source).unwrap();
    assert_eq!(session.print(result.value).unwrap(), expected);
    session.release(result.value).unwrap();
}

#[cfg(light_v8)]
#[test]
fn compiler_owned_data_readers_keep_source_order_origins_and_typed_results() {
    use light_cljs::{
        MacroHost, compile_with_macros,
        compiler_data::{Name, Origin, Source},
    };
    let mut host = MacroHost::new();
    host.register_compiler_data_reader(
        Name::new(Some("test"), "tag"),
        "(let [seen (atom [])] (fn [x] (swap! seen conj x) [x @seen]))",
    )
    .unwrap();
    let source = "#_ #test/tag 1 (let [x #test/tag 2] (if false #test/tag 3 x)) #_ #test/tag 4";
    let js = compile_with_macros(&format!("(pr-str (do {source}))"), &mut host).unwrap();
    let e = Evaluator::new(Default::default()).unwrap();
    // Every read occurs, including a discarded form and an untaken branch.
    assert_eq!(e.string(e.evaluate_js(&js).unwrap()).unwrap(), "[2 [1 2]]");
    let source = Source::new("owned.cljs", "#test/tag 5");
    let form = host.read_compiler_form(&source).unwrap().unwrap();
    assert!(matches!(form.node().origin, Origin::DataReader(_)));
    assert_eq!(form.span().text(), "#test/tag 5");
    assert_eq!(form.span().source().name(), "owned.cljs");
    host.register_compiler_data_reader(Name::new(None, "inst"), "(fn [x] (+ x 10))")
        .unwrap();
    let inst = compile_with_macros("#inst 5", &mut host).unwrap();
    assert_eq!(e.number(e.evaluate_js(&inst).unwrap()).unwrap(), 15.0);
    for handler in [
        "(fn [x] (atom x))",
        "(fn [x] (fn [] x))",
        "(fn [x] (js-obj \"secret\" x))",
    ] {
        host.register_compiler_data_reader(Name::new(None, "bad"), handler)
            .unwrap();
        let input = Source::new("invalid-reader.cljs", "[#bad 1]");
        let error = host.read_compiler_form(&input).unwrap_err();
        assert_eq!(error.span.text(), "#bad 1");
        assert_eq!(error.span.source().name(), "invalid-reader.cljs");
        host.register_compiler_data_reader(Name::new(None, "bad"), "identity")
            .unwrap();
        assert!(host.read_compiler_form(&input).unwrap().is_some());
    }
    let mut discarded = MacroHost::new();
    discarded
        .register_compiler_data_reader(
            Name::new(None, "count"),
            "(let [n (atom 0)] (fn [x] (swap! n inc)))",
        )
        .unwrap();
    assert!(
        discarded
            .read_compiler_form(&Source::new("discard.cljs", "#_ #count 1"))
            .unwrap()
            .is_none()
    );
    let form = discarded
        .read_compiler_form(&Source::new("after.cljs", "#count 1"))
        .unwrap()
        .unwrap();
    assert!(
        matches!(form.data().inspect(), light_cljs::compiler_data::DatumView::Double(n) if n == 2.0)
    );
    let mut bounded = MacroHost::new();
    bounded.limits.generated_nodes = 5;
    bounded
        .register_compiler_data_reader(Name::new(None, "size"), "(fn [x] [x x x])")
        .unwrap();
    let input = Source::new("bounded-reader.cljs", "#size 1");
    assert!(bounded.read_compiler_form(&input).unwrap().is_some());
    let error = bounded.read_compiler_form(&input).unwrap_err();
    assert!(error.message.contains("generated-node limit"));
    assert_eq!(error.span.text(), "#size 1");
    // A fresh compilation resets cumulative output and failed reader state.
    assert!(compile_with_macros("#size 1", &mut bounded).is_ok());
}

#[cfg(light_v8)]
#[test]
fn compiler_data_readers_report_conversion_and_execution_limits() {
    use light_cljs::{
        MacroHost,
        compiler_data::{Name, Source},
    };
    use std::time::Duration;

    let mut host = MacroHost::new();
    host.limits.generated_nodes = 5;
    host.evaluator_options.timeout = Duration::from_secs(2);
    host.register_compiler_data_reader(
        Name::new(None, "bounded"),
        "(fn [x] (case x
                   0 :ok
                   1 [x x x x x]
                   2 (reduce (fn [nested _] [nested]) x (range 129))
                   (loop [] (recur))))",
    )
    .unwrap();

    let read = |host: &mut MacroHost, text: &str| {
        host.read_compiler_form(&Source::new("reader-limits.cljs", text))
    };
    assert!(read(&mut host, "#bounded 0").unwrap().is_some());
    let error = read(&mut host, "#bounded 1").unwrap_err();
    assert!(error.message.contains("node limit at "), "{error}");
    assert_eq!(error.span.text(), "#bounded 1");
    host.limits.generated_nodes = 1000;
    let error = read(&mut host, "#bounded 2").unwrap_err();
    assert!(error.message.contains("depth limit at "), "{error}");
    assert_eq!(error.span.text(), "#bounded 2");
    assert!(read(&mut host, "#bounded 0").unwrap().is_some());
    let error = read(&mut host, "#bounded 3").unwrap_err();
    assert!(error.message.contains("deadline exceeded"), "{error}");
    assert_eq!(error.span.text(), "#bounded 3");
}

#[test]
fn upstream_set_and_walk_witnesses_execute_with_full_setup() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/compiler-libraries");
    let js = compile_namespace("library.upstream", &[root]).unwrap();
    #[cfg(light_v8)]
    {
        let e = Evaluator::new(Default::default()).unwrap();
        assert_eq!(
            e.string(e.evaluate_js(&format!("JSON.stringify({js})")).unwrap())
                .unwrap(),
            "true"
        );
    }
    let mut child = Command::new("node")
        .arg("-")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::Write;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(format!("console.log(JSON.stringify({js}));").as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8(out.stdout).unwrap().trim(), "true");
}

/// At `Advanced` a program keeps only the bundled clojure.set arities it calls
/// and drops a required clojure.walk it never calls, while wrong counts,
/// `apply` and saved values still get the arity errors (light-cljs-986).
#[test]
fn advanced_keeps_only_called_library_arities_and_their_errors() {
    use super::optimization::{execute, readable};
    use light_cljs::analyzer::AnalysisOptions;
    use light_cljs::compiler::analyze_namespace_entry;
    use light_cljs::{MacroHost, OptimizationLevel, prepare_application};
    let sources = Sources::new();
    let advanced = |body: &str| {
        sources.write(
            "probe/main.cljs",
            &format!("(ns probe.main (:require [clojure.set :as s] [clojure.walk :as w])) {body}"),
        );
        let program = analyze_namespace_entry(
            "probe.main",
            std::slice::from_ref(sources.0.path()),
            &mut MacroHost::new(),
            AnalysisOptions::default(),
            &mut Vec::new(),
        )
        .unwrap();
        let js = prepare_application(&program, readable(OptimizationLevel::Advanced)).emit_js();
        let value = execute(&format!("console.log(JSON.stringify({js}));"));
        (value.trim().to_owned(), js)
    };
    let (value, js) = advanced("(pr-str (s/difference #{1 2 3} #{2}))");
    assert_eq!(value, r##""#{1 3}""##);
    // No dispatcher, no other method of any public function (the throwing
    // ones included), no other clojure.set function, no walk engine.
    for absent in [
        "maxFixedArity",
        "Invalid arity: 0",
        "expected map or nil",
        "walk depth limit",
    ] {
        assert!(!js.contains(absent), "{absent}\n{js}");
    }
    // The engine, made at the first walk, keeps no core function that was
    // redefined during that walk.
    let first_walk_redefs = "(pr-str [(with-redefs [cljs.core/next (constantly nil)] \
         (w/prewalk identity [1 2])) (w/prewalk identity [3 4])])";
    assert_eq!(advanced(first_walk_redefs).0, r#""[[1] [3 4]]""#);
    both(first_walk_redefs, r#""[[1] [3 4]]""#);
    let (value, _) = advanced(
        r#"(let [caught (fn [f] (try (f) "none" (catch js/Error e (.-message e))))
                 saved [s/select w/prewalk]]
      #js [(caught #(s/difference)) (caught #(s/select odd?)) (caught #(s/select odd? #{} 1))
           (caught #(s/join #{} #{} {} 1)) (caught #(apply s/intersection []))
           (caught #((first saved) odd?)) (caught #(w/walk identity identity))
           (caught #((second saved) identity [1] 2)) (caught #(w/keywordize-keys {} {}))
           (pr-str (s/difference #{1 2 3} #{2} #{3}))
           (pr-str (w/postwalk-replace {1 2} [1 [1]]))])"#,
    );
    assert_eq!(
        value,
        r##"["Invalid arity: 0","Invalid arity: 1","Invalid arity: 3","Invalid arity: 4","Invalid arity: 0","Invalid arity: 1","Invalid arity: 2","Invalid arity: 3","Invalid arity: 2","#{1}","[2 [2]]"]"##
    );
}
