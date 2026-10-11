//! Canonical library values run in both engines; macros demand the same sources.
use light_cljs::{
    compile_namespace,
    evaluator::{Evaluator, EvaluatorOptions},
};
use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Sources(PathBuf);
impl Sources {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "light-library-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
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
        compile_namespace("probe.main", std::slice::from_ref(&self.0)).unwrap()
    }
}
impl Drop for Sources {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn both(body: &str, expected: &str) {
    let sources = Sources::new();
    let js = sources.compile(body);
    let evaluator = Evaluator::new(Default::default()).unwrap();
    let value = evaluator
        .evaluate_js(&format!("JSON.stringify({js})"))
        .unwrap();
    assert_eq!(evaluator.string(value).unwrap(), expected, "QuickJS {body}");
    let out = Command::new("node")
        .args(["--eval", &format!("console.log(JSON.stringify({js}));")])
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
    let js = compile_namespace("probe.main", std::slice::from_ref(&sources.0)).unwrap();
    let e = Evaluator::new(Default::default()).unwrap();
    assert_eq!(
        e.string(e.evaluate_js(&format!("JSON.stringify({js})")).unwrap())
            .unwrap(),
        "true"
    );
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
    let sources = Sources::new();
    let bad = sources.compile("(try (w/prewalk (fn [x] [x]) 1) (catch js/Error e (.-message e)))");
    let e = Evaluator::new(EvaluatorOptions::default()).unwrap();
    assert!(e
        .string(e.evaluate_js(&bad).unwrap())
        .unwrap()
        .contains("walk depth limit"));
    assert_eq!(e.number(e.evaluate_js("40+2").unwrap()).unwrap(), 42.0);
    let infinite = sources.compile("(w/postwalk identity (range))");
    let e = Evaluator::new(EvaluatorOptions {
        poll_budget: 1000,
        ..Default::default()
    })
    .unwrap();
    assert!(e.evaluate_js(&infinite).is_err());
}

#[path = "support/sci_protocol.rs"]
#[allow(dead_code)]
mod native_fixture;
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
    let cases = native_fixture::cases(include_str!("fixtures/sci-outcomes.tsv")).unwrap();
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
    let js = compile_namespace("probe.main", std::slice::from_ref(&sources.0)).unwrap();
    let e = Evaluator::new(Default::default()).unwrap();
    let observations = e
        .string(e.evaluate_js(&format!("JSON.stringify({js})")).unwrap())
        .unwrap();
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

#[test]
fn compiler_owned_data_readers_keep_source_order_origins_and_typed_results() {
    use light_cljs::{
        compile_with_macros,
        compiler_data::{Name, Origin, Source},
        MacroHost,
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
    assert_eq!(
        e.number(
            e.evaluate_js(&compile_with_macros("#inst 5", &mut host).unwrap())
                .unwrap()
        )
        .unwrap(),
        15.0
    );
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
    assert!(discarded
        .read_compiler_form(&Source::new("discard.cljs", "#_ #count 1"))
        .unwrap()
        .is_none());
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

#[test]
fn upstream_set_and_walk_witnesses_execute_with_full_setup() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/compiler-libraries");
    let js = compile_namespace("library.upstream", &[root]).unwrap();
    let e = Evaluator::new(Default::default()).unwrap();
    assert_eq!(
        e.string(e.evaluate_js(&format!("JSON.stringify({js})")).unwrap())
            .unwrap(),
        "true"
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
    assert_eq!(String::from_utf8(out.stdout).unwrap().trim(), "true");
}
