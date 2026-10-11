//! Deterministic cross-products of language features and emission contexts.
//! These are independent probes, never imported upstream assertion coverage.
use super::*;

#[test]
fn doto_evaluates_its_target_once_and_preserves_order_and_identity() {
    let source = r#"(let [seen (atom []) target #js []
                         record (fn [x] (swap! seen conj :function) (.push x 3))
                         result (doto (do (swap! seen conj :target) target)
                                  (.push (do (swap! seen conj :first) 1))
                                  (.push (do (swap! seen conj :second) 2)) record)]
                     (array (identical? result target) target (to-array (map name @seen))
                       (identical? target (doto target))))"#;
    let javascript = compile(source).unwrap();
    assert_eq!(
        node(&format!("console.log(JSON.stringify({javascript}));")),
        "[true,[1,2,3],[\"target\",\"first\",\"second\",\"function\"],true]\n"
    );
}

#[test]
#[ignore = "requires Clojure CLI, Java, Node, and pinned compiler references"]
fn late_language_helpers_match_pinned_compilers() {
    let cases: Vec<_> = include_str!("../fixtures/compiler-core-helpers.tsv")
        .lines()
        .filter(|line| line.starts_with("language-helper-"))
        .map(|line| {
            let fields: Vec<_> = line.split('\t').collect();
            Case {
                id: fields[0].into(),
                source: fields[1].into(),
                outcome: format!(r#"{{"status":"returned","value":{}}}"#, fields[2]),
                trace: "[]".into(),
                stdout: "\"\"".into(),
            }
        })
        .collect();
    assert_cases(&cases, &compiled_cases(&cases), "compiled JavaScript");
    let input = TempDir::new("language-helpers-input");
    let path = input.0.join("cases.tsv");
    fs::write(
        &path,
        cases
            .iter()
            .map(|case| format!("{}\t{}\n", case.id, case.source))
            .collect::<String>(),
    )
    .unwrap();
    for reference in reference_results(
        "language-helpers",
        &["cases", path.to_str().unwrap()],
        include_str!("execution.js"),
    ) {
        assert_cases(&cases, &reference.output, &reference.identity);
    }
}

#[test]
fn run_bang_stops_at_reduced_even_on_an_unbounded_input() {
    let javascript = compile("(let [seen (atom []) result (run! (fn [x] (swap! seen conj x) (when (= x 2) (reduced nil))) (range))] (array result (to-array @seen)))").unwrap();
    assert_eq!(
        node(&format!("console.log(JSON.stringify({javascript}));")),
        "[null,[0,1,2]]\n"
    );
}

#[test]
fn completion_helpers_preserve_lazy_stops_metadata_and_callback_order() {
    for (source, expected) in [
        (
            "(let [seen (atom []) k (fn [x] (swap! seen conj x) (mod x 2))] (array (min-key k 1 3) (max-key k 2 4) (to-array @seen)))",
            "[3,4,[1,3,2,4]]\n",
        ),
        (
            "(to-array (reductions (fn [acc x] (if (= x 3) (reduced acc) (+ acc x))) 0 (range)))",
            "[0,0,1,3,3]\n",
        ),
        (
            "(to-array (take 3 (distinct (cycle [1 1 2 2 3]))))",
            "[1,2,3]\n",
        ),
        (
            "(array (distinct? 1 2 3) (distinct? 1 2 1) (apply distinct? [1 2 3]) (distinct? nil nil))",
            "[true,false,true,false]\n",
        ),
        (
            "(let [v (with-meta [1 2 3] {:x 9}) r (replace {1 nil 2 false} v)] (array (vector? r) (:x (meta r)) (to-array r) (to-array (replace {1 nil} '(1 2)))))",
            "[true,9,[null,false,3],[null,2]]\n",
        ),
        (
            "(array (to-array (sort > [1 3 2])) (to-array (sort-by count [\"aaa\" \"b\" \"cc\" \"d\"])) (:x (meta (sort (with-meta [3 2 1] {:x 9})))))",
            "[[3,2,1],[\"b\",\"d\",\"cc\",\"aaa\"],9]\n",
        ),
        (
            "(array (to-array (subvec [1 2 3] -0.9 3.9)) (compare (uuid \"a\") (uuid \"b\")) (compare (js/Date. 1) (js/Date. 2)) (= js/-Infinity ##-Inf) (exists? cljs.core.first))",
            "[[1,2,3],-1,-1,true,true]\n",
        ),
    ] {
        let javascript = compile(source).unwrap_or_else(|error| panic!("{source}: {error}"));
        assert_eq!(
            node(&format!("console.log(JSON.stringify({javascript}));")),
            expected,
            "{source}"
        );
    }
}

fn combinations() -> Vec<Case> {
    let forms = [
        (
            "overload",
            "(let [f (fn ([] 0) ([x] (+ 6 x)) ([x y & more] (reduce + (+ x y) more)))] (f (observe 1)))",
        ),
        (
            "apply",
            "(let [f (fn ([x] (+ 6 x)) ([x y & more] (reduce + (+ x y) more)))] (apply f (list (observe 1) 2 4)))",
        ),
        (
            "recur",
            "(loop [n 3 total (observe 1)] (if (zero? n) total (recur (dec n) (+ total n))))",
        ),
        (
            "letfn",
            "(letfn [(f [n] (if (zero? n) (observe 1) (g (dec n)))) (g [n] (f n))] (+ 6 (f 3)))",
        ),
        (
            "destructure",
            "(let [[x & more] [(observe 1) 2 4] {:keys [offset] :or {offset 0}} {}] (+ offset x (reduce + more)))",
        ),
        (
            "state",
            "(let [a (atom 0)] (add-watch a :w (fn [_ _ before after] (observe after))) (swap! a inc) (+ 6 @a))",
        ),
        (
            "metadata",
            "(let [x (with-meta [(observe 1)] {:offset 6})] (+ (first x) (:offset (meta x))))",
        ),
        (
            "reify",
            "(let [n (observe 1) f (reify IFn (-invoke [_ x] (+ n x)))] (f 6))",
        ),
        (
            "record",
            "(defrecord Entry [n]) (let [x (->Entry (observe 1))] (+ 6 (:n (assoc x :tag true))))",
        ),
        (
            "protocol",
            "(let [x (reify IReduce (-reduce [_ f init] (f init (observe 1))))] (reduce + 6 x))",
        ),
        (
            "lazy",
            "(reduce + (map (fn [x] (if (= x 1) (observe x) x)) (list 1 2 4)))",
        ),
        (
            "exception",
            "(try (throw (ex-info \"caught\" {:n (observe 1)})) (catch js/Error e (+ 6 (:n (ex-data e)))))",
        ),
    ];
    let mut cases = Vec::new();
    for (feature, source) in forms {
        for failing in [false, true] {
            // The outer failing path runs only after the feature body completed;
            // nested catches cannot accidentally consume this expected failure.
            let body = if failing {
                format!("(do {source} (throw (js/Error. \"combination failure\")))")
            } else {
                format!("(do {source})")
            };
            let wrappers = [
                ("return", body.clone()),
                ("initializer", format!("(let [v {body}] v)")),
                ("argument", format!("(identity {body})")),
                ("discard", format!("(do {body} 7)")),
                (
                    "selected",
                    format!("(if true {body} (throw (js/Error. \"unselected\")))"),
                ),
                ("closure", format!("((fn [] {body}))")),
                (
                    "recur",
                    format!("(loop [i 1] (if (zero? i) {body} (recur (dec i))))"),
                ),
                ("shadow", format!("(let [v 100] (+ 0 (let [v {body}] v)))")),
                (
                    "finally",
                    format!("(try {body} (finally (observe \"finally\")))"),
                ),
            ];
            for (context, body) in wrappers {
                cases.push(Case {
                    id: format!(
                        "language-{feature}-{context}-{}",
                        if failing { "throw" } else { "value" }
                    ),
                    source: format!("(do (js/console.log \"combination\") {body})"),
                    outcome: if failing {
                        r#"{"status":"threw","error":{"category":"error"}}"#.into()
                    } else {
                        r#"{"status":"returned","value":7}"#.into()
                    },
                    trace: if context == "finally" {
                        "[1,\"finally\"]"
                    } else {
                        "[1]"
                    }
                    .into(),
                    stdout: "\"combination\\n\"".into(),
                });
            }
        }
    }
    assert_eq!(cases.len(), 216);
    cases
}

fn assert_cases(cases: &[Case], actual: &str, label: &str) {
    for (case, actual) in cases.iter().zip(outcomes(actual, cases).unwrap()) {
        let expected = format!(
            "{{\"id\":{},\"outcome\":{},\"trace\":{},\"stdout\":{}}}",
            json_string(&case.id),
            case.outcome,
            case.trace,
            case.stdout
        );
        assert_eq!(actual, expected, "{label}: {}: {}", case.id, case.source);
    }
}

#[test]
fn bounded_language_combinations_execute_values_effects_stdout_and_failures() {
    let cases = combinations();
    assert_cases(&cases, &compiled_cases(&cases), "compiled JavaScript");
}

#[test]
#[ignore = "requires Clojure CLI, Java, Node, and pinned compiler references"]
fn bounded_language_combinations_match_pinned_compilers() {
    let cases = combinations();
    let input = TempDir::new("language-combinations-input");
    let path = input.0.join("cases.tsv");
    fs::write(
        &path,
        cases
            .iter()
            .map(|case| format!("{}\t{}\n", case.id, case.source))
            .collect::<String>(),
    )
    .unwrap();
    assert_cases(&cases, &compiled_cases(&cases), "compiled JavaScript");
    for reference in reference_results(
        "language-combinations",
        &["cases", path.to_str().unwrap()],
        include_str!("execution.js"),
    ) {
        assert_cases(&cases, &reference.output, &reference.identity);
        eprintln!(
            "{}: all {} deterministic language combinations matched",
            reference.identity,
            cases.len()
        );
    }
}

const INTEGRATED_EXPECTED: &str = include_str!("../fixtures/namespaces/integrated/expected.json");

fn integrated_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/namespaces/integrated")
}

fn integrated_driver(function: &str) -> String {
    harness_script(&[("integrated-language", "integrated.main/run", function)])
}

fn integrated_local() -> String {
    let javascript =
        light_cljs::compile_namespace("integrated.main", &[integrated_root()]).unwrap();
    assert_eq!(
        javascript.matches(" = // Original bounded runtime").count(),
        1
    );
    let javascript = javascript.replacen(
        " = // Original bounded runtime",
        " = globalThis.lightProgramRuntime = // Original bounded runtime",
        1,
    );
    node(&format!(
        "globalThis.lightInspectValue = value => lightProgramRuntime.inspect(value);\n{}",
        integrated_driver(&format!(
            "observe => {{ const run = {javascript}; return run(observe); }}"
        ))
    ))
}

#[test]
fn integrated_language_program_combines_cljs_cljc_and_compiled_macros() {
    assert_eq!(integrated_local(), INTEGRATED_EXPECTED);
}

#[test]
#[ignore = "requires Clojure CLI, Java, Node, and pinned compiler references"]
fn integrated_language_program_matches_pinned_compilers() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/reference-namespaces/integrated");
    assert_eq!(integrated_local(), INTEGRATED_EXPECTED);
    let driver = format!(
        "globalThis.lightInspectValue = value => lightProgramInspectValue(value);\n{}",
        integrated_driver("observe => { loadProgram(); return lightProgramRun(observe); }")
    );
    for reference in reference_results_with_macro_roots(
        "integrated-language",
        &["program", root.to_str().unwrap(), "integrated.main", "run"],
        &driver,
        std::slice::from_ref(&root),
    ) {
        assert_eq!(
            reference.output, INTEGRATED_EXPECTED,
            "{}",
            reference.identity
        );
        eprintln!(
            "{}: complete integrated result, effects and stdout matched",
            reference.identity
        );
    }
}
