//! Every native string fixture is retained, with explicit JavaScript departures.
use light_cljs::{compile_namespace, evaluator::Evaluator};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
};
#[path = "support/sci_protocol.rs"]
#[allow(dead_code)]
mod native;
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Sources(PathBuf);
impl Sources {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "light-string-fixtures-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn write(&self, name: &str, source: &str) {
        let path = self.0.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, source).unwrap();
    }
    fn compile(&self, source: &str) -> Result<String, light_cljs::NamespaceCompileError> {
        self.write("user.cljs", source);
        compile_namespace("user", std::slice::from_ref(&self.0))
    }
}
impl Drop for Sources {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn literal(value: &native::Value) -> String {
    use native::Value as V;
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
        V::Float(bits) => {
            let x = f64::from_bits(*bits);
            if x.is_nan() {
                "##NaN".into()
            } else if x == f64::INFINITY {
                "##Inf".into()
            } else if x == f64::NEG_INFINITY {
                "##-Inf".into()
            } else {
                x.to_string()
            }
        }
        V::Ratio(n, d) => format!("(/ {n} {d})"),
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
        _ => panic!("unsupported fixture value {value:?}"),
    }
}
fn adapted_source(id: &str, source: &str) -> String {
    let source = match id {
        "sci-string-join-adapted" => "(string/join \"-\" [1 2 3])",
        "sci-string-namespace-alias-publics" => {
            "[(s/join \"-\" [1 2]) (= \"clojure.string\" (ns-name (:ns (meta #'s/join)))) (= #'s/join (get (ns-publics 'clojure.string) 'join)) (contains? (ns-publics 'clojure.string) 'escape)]"
        }
        "sci-string-gensym-arity" => "(apply gensym [\"a\" \"b\"])",
        "sci-string-namespace-repeat-require" => {
            "(let [before (ns-publics 'clojure.string)] [(= before (ns-publics 'clojure.string)) (s/join \"-\" [1 2])])"
        }
        _ => source,
    };
    source
        .replace("-1/2", "(/ -1 2)")
        .replace("3/2", "(/ 3 2)")
        .replace("1/2", "(/ 1 2)")
}
// Explicit non-printing host outcomes. Printing expectations are independently
// verified against both pinned CLJS references; native outcomes stay separate.
const OVERRIDES: &[(&str, &str, &str)] = &[
    (
        "sci-string-join-values",
        r#"[:returned ["falsexa" ",1,:x" "1null2" "1-2" "1:x2"]]"#,
        "Canonical StringBuffer separator conversion; original SCI nil-separator expectation remains historical",
    ),
    (
        "sci-string-str-scalars",
        r#"[:returned "truefalse12-30.5:xn/x ok"]"#,
        "JS ratio/character representation",
    ),
    (
        "sci-string-str-collections",
        r#"[:returned "[\"x\" \"\\n\" nil](1 2){:a \"x\"}#{:x}"]"#,
        "JS character representation",
    ),
    (
        "sci-string-name-type",
        r#"[:returned "a"]"#,
        "JS character/string identity",
    ),
    (
        "sci-string-keyword-split",
        r#"[:returned [["a" nil ":a/b/c"] ["/" nil ":/"] ["x" "" ":/x"] ["" "a" ":a/"] ["" nil ":"] ["x" "" ":/x"]]]"#,
        "JS pinned keyword split",
    ),
    (
        "sci-string-keyword-type",
        "[:returned :a/b]",
        "JS keyword argument conversion",
    ),
    (
        "sci-string-symbol-split",
        r#"[:returned [["b/c" "a" "a/b/c"] ["/" nil "/"] ["/x" nil "/x"] ["" "a" "a/"] ["" nil ""] ["x" "" "/x"]]]"#,
        "JS pinned symbol slash handling",
    ),
    (
        "sci-string-case-coercion",
        "[:error]",
        "JS strict case receiver",
    ),
    (
        "sci-string-trim-unicode",
        r#"[:returned [["" true] ["" true] ["" true] ["" true] ["\u0085" false] ["" true] ["\u001f" false] ["\u0000" false]]]"#,
        "JS whitespace membership",
    ),
    (
        "sci-string-blank-type",
        "[:returned false]",
        "JS safe string conversion",
    ),
    (
        "sci-string-includes-type",
        "[:returned true]",
        "JS character/string identity",
    ),
    (
        "sci-string-index-empty",
        "[:returned [0 3 0 3 0 0]]",
        "JS index clamping",
    ),
    (
        "sci-string-index-coerce",
        "[:returned [1 nil 1]]",
        "JS index conversion without int32 narrowing",
    ),
    (
        "sci-string-index-type",
        "[:returned nil]",
        "JS search argument conversion",
    ),
    (
        "sci-string-replace-char-type",
        r#"[:returned "xbx"]"#,
        "JS character/string identity",
    ),
    (
        "sci-string-replace-string-type",
        r#"[:returned "xbx"]"#,
        "JS character/string identity",
    ),
    (
        "sci-string-escape-map",
        r#"[:returned ["XXbXX" "falseb0" "x:k[1]"]]"#,
        "JS documented non-nil escape result",
    ),
    (
        "sci-string-index-character-wrap",
        "[:returned [nil 0 nil 0]]",
        "JS index conversion without int32 narrowing",
    ),
    (
        "sci-string-index-empty-wrap",
        "[:returned [3 3 3 3]]",
        "JS index conversion without int32 narrowing",
    ),
    (
        "sci-string-subs-negative",
        r#"[:returned "abc"]"#,
        "JS substring clamping",
    ),
    (
        "sci-string-subs-too-long",
        r#"[:returned "abc"]"#,
        "JS substring clamping",
    ),
    (
        "sci-string-subs-reversed",
        r#"[:returned "b"]"#,
        "JS substring swaps reversed endpoints",
    ),
];
fn printing_expected(id: &str) -> Option<(&'static str, &'static str)> {
    include_str!("fixtures/compiler-native-printing.tsv")
        .lines()
        .filter(|line| !line.starts_with('#'))
        .find_map(|line| {
            let mut fields = line.split('\t');
            let name = fields.next()?;
            let source = fields.next()?;
            let expected = fields.next()?;
            assert!(fields.next().is_none(), "invalid printing fixture {name}");
            (name == id).then_some((source, expected))
        })
}
fn expected(id: &str, outcome: &native::Outcome) -> String {
    if let Some((_, expected)) = printing_expected(id) {
        return expected.into();
    }
    if let Some((_, expected, _)) = OVERRIDES.iter().find(|(name, _, _)| *name == id) {
        return (*expected).into();
    }
    match outcome {
        native::Outcome::Returned(value) => format!("[:returned {}]", literal(value)),
        native::Outcome::Error { .. } => "[:error]".into(),
    }
}
fn node(script: &str) -> String {
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
        .write_all(script.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}
#[test]
fn all_131_native_string_bodies_execute_with_canonical_printing_evidence() {
    let cases = native::cases(include_str!("fixtures/sci-outcomes.tsv")).unwrap();
    let cases = cases
        .iter()
        .filter(|c| c.id.starts_with("sci-string-"))
        .collect::<Vec<_>>();
    assert_eq!(cases.len(), 131);
    let sources = Sources::new();
    let mut failures = Vec::new();
    let mut scripts = Vec::new();
    let mut ids = Vec::new();
    let mut printing_bodies = 0;
    for case in cases {
        assert!(
            case.stdout.is_empty() && case.stderr.is_empty(),
            "{}",
            case.id
        );
        let body = adapted_source(case.id, case.source);
        let effects = if case.id == "sci-string-escape-surrogates" {
            "[0 0]".into()
        } else {
            format!(
                "[{}]",
                case.effects
                    .iter()
                    .map(literal)
                    .collect::<Vec<_>>()
                    .join(" ")
            )
        };
        let repeated_require = if case.id == "sci-string-namespace-repeat-require" {
            "(require 'clojure.string)"
        } else {
            ""
        };
        let source = format!(
            "(ns user (:require [clojure.string :as s] [clojure.string :as string])) {repeated_require} (def effects (atom [])) (defn observe [x] (swap! effects conj x) x) (def actual (try [:returned (do {body})] (catch :default e [:error]))) #js [(= actual {}) (= @effects {effects}) (pr-str actual) (pr-str @effects)]",
            expected(case.id, &case.outcome)
        );
        if let Some((canonical_source, _)) = printing_expected(case.id) {
            assert_eq!(body, canonical_source, "complete printing body {}", case.id);
            printing_bodies += 1;
        }
        let javascript = match sources.compile(&source) {
            Ok(js) => js,
            Err(error) => {
                failures.push(format!("compile {}: {error}", case.id));
                continue;
            }
        };
        let e = Evaluator::new(Default::default()).unwrap();
        let observation = e
            .string(
                e.evaluate_js(&format!("JSON.stringify({javascript})"))
                    .unwrap(),
            )
            .unwrap();
        if !observation.starts_with("[true,true,") {
            failures.push(format!("QuickJS {}: {observation}", case.id));
        }
        scripts.push(format!("console.log(JSON.stringify({javascript}));"));
        ids.push((case.id, observation));
    }
    for (group, expected) in scripts.chunks(8).zip(ids.chunks(8)) {
        let observations = node(&group.join("\n"));
        for ((id, quickjs), node) in expected.iter().zip(observations.lines()) {
            assert_eq!(node, quickjs, "Node/QuickJS {id}");
            if !node.starts_with("[true,true,") {
                failures.push(format!("Node {id}: {node}"));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "native string migration differences:\n{}",
        failures.join("\n")
    );
    assert_eq!(ids.len(), 131);
    assert_eq!(printing_bodies, 58);
    assert_eq!(OVERRIDES.len(), 22);
    for (id, _, reason) in OVERRIDES {
        assert!(
            ids.iter().any(|(native_id, _)| native_id == id),
            "stale override {id}: {reason}"
        );
    }
    println!(
        "Native string evidence: {} executed; {} canonical printing bodies",
        ids.len(),
        printing_bodies
    );
}

#[test]
fn native_printing_sources_and_canonical_expectations_preserve_provenance() {
    let output = Command::new("python3")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args(["-B", "tests/reference/compiler-native-printing.py"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[ignore = "requires Clojure CLI, Java, Node, and both pinned CLJS references"]
fn all_58_native_printing_bodies_match_both_pinned_cljs_references() {
    let output = Command::new("python3")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args(["-B", "tests/reference/compiler-native-printing.py", "--run"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    println!("{}", String::from_utf8_lossy(&output.stdout));
}

#[test]
fn verbatim_pinned_upstream_string_api_and_capitalization_property_execute() {
    let sources = Sources::new();
    // Test.check is replaced only at the generation boundary. The original
    // property/setup and API test bodies are kept verbatim in the fixture.
    let seeded = literal(&native::Value::String(
        include_str!("fixtures/compiler-string-property-samples.json")
            .encode_utf16()
            .collect(),
    ));
    let generators = r#"(ns fixture.generators (:refer-clojure :exclude [vector not-empty]))
      (def seen (atom 0))
      (defn choose [low high] (range low (inc high)))
      (defn vector [codes]
        (map vec (js* "JSON.parse(~{}).map(value => value.split('').map(ch => ch.charCodeAt(0)))" SOURCE_SAMPLES)))
      (defn not-empty [values] (filter seq values))
      (defn fmap [f values] (map (fn [value] (swap! seen inc) (f value)) values))"#.replace("SOURCE_SAMPLES", &seeded);
    sources.write("fixture/generators.cljs", &generators);
    sources.write(
        "fixture/properties.cljs",
        r#"(ns fixture.properties)
      (defmacro for-all [bindings & body]
        (list 'every? (list* 'fn [(first bindings)] body) (second bindings)))"#,
    );
    let setup = r#"(ns user (:require [clojure.string :as s]
                          [fixture.generators :as gen]
                          [fixture.properties :as prop]))
      (def checks (atom []))
      (defmacro deftest [name & body] (list* 'defn name [] body))
      (defmacro testing [label & body] (cons 'do body))
      (defmacro is [form & message] (let [result (gensym "assertion")]
        (list 'let [result form] (cons 'do message) (list 'swap! 'checks 'conj result) result)))
      (defmacro defspec [name body] (list 'defn name [] body))
    "#;
    let js = sources
        .compile(&format!(
            "{setup}\n{}\n(test-api) #js [(every? true? @checks) (count @checks) (test-cljs-2300) @gen/seen]",
            include_str!("fixtures/compiler-upstream-string-tests.cljs")
        ))
        .unwrap();
    let e = Evaluator::new(light_cljs::evaluator::EvaluatorOptions {
        timeout: std::time::Duration::from_secs(60),
        ..Default::default()
    })
    .unwrap();
    let observation = e
        .string(e.evaluate_js(&format!("JSON.stringify({js})")).unwrap())
        .unwrap();
    assert_eq!(observation, "[true,95,true,1000]");
    assert_eq!(
        node(&format!("console.log(JSON.stringify({js}));")).trim(),
        observation
    );
    println!("Upstream API/property observations: {observation}");
}

#[test]
#[ignore = "requires Clojure CLI, Java, Node, and both pinned CLJS references"]
fn full_upstream_bodies_pass_both_pinned_cljs_references() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let checkout = root.join("research/clojurescript");
    let revision = "a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e";
    let mut seeded_samples = None;
    for (kind, identity, dependency, checkout_arg) in [
        (
            "release",
            "1.12.134",
            "{:mvn/version \"1.12.134\"}".to_string(),
            "-".to_string(),
        ),
        (
            "checkout",
            revision,
            format!("{{:local/root {:?}}}", checkout),
            checkout.to_string_lossy().into_owned(),
        ),
    ] {
        let output_dir = std::env::temp_dir().join(format!(
            "light-cljs-string-oracle-{kind}-{}",
            std::process::id()
        ));
        fs::create_dir_all(&output_dir).unwrap();
        let out=Command::new("clojure").current_dir(&root).args([
            "-J-Xmx1g", "-Srepro", "-Sdeps", &format!("{{:deps {{org.clojure/clojurescript {dependency} org.clojure/test.check {{:mvn/version \"1.1.1\"}}}}}}"),
            "-M", "tests/reference/execution.clj", kind, identity, &checkout_arg,
        ]).arg(&output_dir).args(["program", "tests/fixtures/compiler-strings-reference", "reference.main", "run"]).output().unwrap();
        assert!(
            out.status.success(),
            "{kind}: {}\n{}",
            String::from_utf8_lossy(&out.stderr),
            String::from_utf8_lossy(&out.stdout)
        );
        let settings = String::from_utf8(out.stdout).unwrap();
        assert!(
            settings.contains(&format!("REFERENCE IDENTITY {kind} {identity}")),
            "{settings}"
        );
        assert!(
            settings.contains(
                "REFERENCE SETTINGS {:optimizations :none, :static-fns false, :def-emits-var false}"
            ),
            "{settings}"
        );
        let output = node(&format!(
            "process.chdir({:?}); require({:?}); console.log(JSON.stringify(globalThis.lightProgramRun())); console.log(JSON.stringify(globalThis.stringPropertySamples));",
            output_dir,
            output_dir.join("reference.js")
        ));
        let mut lines = output.lines();
        assert_eq!(
            lines.next(),
            Some("[95,0,0,true,1000,123456]"),
            "{kind}: API/property outcome differs"
        );
        let samples = lines.next().unwrap().to_owned();
        if let Some(first) = &seeded_samples {
            assert_eq!(first, &samples, "reference seeded sample mismatch");
        } else {
            seeded_samples = Some(samples.clone());
        }
        fs::write(output_dir.join("property-samples.json"), &samples).unwrap();
        assert_eq!(
            samples,
            include_str!("fixtures/compiler-string-property-samples.json"),
            "pinned property sample corpus changed"
        );
        println!(
            "Pinned string reference {kind} {identity}: 95 API passes, 1000 seeded property passes"
        );
        fs::remove_dir_all(output_dir).unwrap();
    }
}

#[test]
fn upstream_local_and_reference_witnesses_retain_identical_full_bodies() {
    let local = include_str!("fixtures/compiler-upstream-string-tests.cljs");
    let reference = include_str!("fixtures/compiler-strings-reference/reference/main.cljs");
    let local_body = &local[local.find("(deftest test-api").unwrap()..];
    let reference_body = &reference
        [reference.find("(deftest test-api").unwrap()..reference.find("(defn run []").unwrap()];
    assert_eq!(local_body.trim(), reference_body.trim());
    assert!(local_body.contains("(defspec test-cljs-2300"));
    assert!(local_body.contains("(gen/not-empty (gen/vector (gen/choose 0x20 0x7E)))"));
}
