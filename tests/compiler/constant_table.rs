//! The `Advanced` constant table and keyword interning (light-cljs-208;
//! docs/advanced-optimization.md, Phase 1 step 7 and "Phase 1 constant table
//! extension points").
//!
//! At `Advanced`, each distinct keyword and quoted symbol literal is declared
//! once, `const k<n> = rt$keyword(...)`, after the runtime slice, and sites read
//! `k<n>`. The runtime interns literal keywords at every level, so equal
//! literals are one object; keywords built from data at run time are fresh and
//! equal them by spelling. Later passes (type inference, light-cljs-210) rely on
//! exactly the identities pinned here.
use super::optimization::execute;
use light_cljs::{
    ApplicationOptions, EvalOptions, EvalSession, OptimizationLevel, Program, analyze, parse,
    prepare_application,
};

fn program(source: &str) -> Program {
    analyze(source, &parse(source).unwrap()).unwrap_or_else(|error| panic!("{source}\n{error}"))
}

fn emit(program: &Program, level: OptimizationLevel) -> String {
    prepare_application(program, ApplicationOptions::new(level)).emit_js()
}

/// The program prints `expected` (its value, through `console.log`) at every
/// level.
fn every_level(source: &str, expected: &str) {
    let program = program(source);
    for level in OptimizationLevel::ALL {
        assert_eq!(
            execute(&format!("console.log({});", emit(&program, level))),
            format!("{expected}\n"),
            "at {level}: {source}"
        );
    }
}

/// The `const k<n> = ...;` declarations of `Advanced` output, in order.
fn declarations(js: &str) -> Vec<&str> {
    js.lines()
        .filter_map(|line| line.strip_prefix("  const k"))
        .filter(|rest| rest.starts_with(|c: char| c.is_ascii_digit()))
        .collect()
}

#[test]
fn advanced_declares_each_distinct_literal_once_after_the_runtime() {
    let source = r#"
      (defn tag [x] (case x :a :alpha :b :beta :other))
      (def table {:a 1 :b 2 'sym 3 :ns/a 4})
      (pr-str [(tag :a) (tag :b) (tag :c) (get table :a) (get table 'sym) 'sym
               (meta '^:m sym2) :ns/a (keyword "ns" "a") '(sym :a)])"#;
    every_level(
        source,
        "[:alpha :beta :other 1 3 sym {:m true} :ns/a :ns/a (sym :a)]",
    );
    let program = program(source);
    let advanced = emit(&program, OptimizationLevel::Advanced);
    // One declaration per distinct literal, numbered in first-use order; an
    // entry's parts come first.
    assert_eq!(
        declarations(&advanced),
        [
            r#"0 = rt$keyword(null, "alpha");"#,
            r#"1 = rt$keyword(null, "beta");"#,
            r#"2 = rt$keyword(null, "other");"#,
            r#"3 = rt$keyword(null, "a");"#,
            r#"4 = rt$keyword(null, "b");"#,
            r#"5 = rt$symbol(null, "sym");"#,
            r#"6 = rt$keyword("ns", "a");"#,
            r#"7 = rt$mapFromArray([k3, 1, k4, 2, k5, 3, k6, 4]);"#,
            r#"8 = rt$keyword(null, "c");"#,
            r#"9 = rt$symbol(null, "sym2");"#,
            r#"10 = rt$keyword(null, "m");"#,
            r#"11 = rt$mapFromArray([k10, true]);"#,
            r#"12 = rt$withMeta(k9, k11);"#,
            r#"13 = rt$listFromArray([k5, k3]);"#,
        ]
    );
    // The table follows the runtime slice and precedes the program, and no
    // site constructs a literal. Nothing reaches reflection, so there is no
    // namespace registration (which would follow the table).
    let table = advanced.find("  const k0 = ").unwrap();
    assert!(advanced.find("  const [rt$").unwrap() < table);
    assert!(table < advanced.find("  let g").unwrap());
    assert!(!advanced.contains("registerReaderNamespace"));
    assert_eq!(advanced.matches("rt$keyword(").count(), 8);
    assert_eq!(advanced.matches("rt$symbol(").count(), 2);
    // Constant collections are entries too (light-cljs-218), with their
    // keywords and symbols declared first: the map, the quoted list, and the
    // quoted symbol with metadata, which is `with-meta` of the shared symbol.
    assert_eq!(advanced.matches("rt$withMeta(").count(), 1);
    assert_eq!(advanced.matches("rt$listFromArray(").count(), 1);
    // Bundler output declares the same table.
    let esm = prepare_application(
        &program,
        ApplicationOptions::new(OptimizationLevel::Advanced),
    )
    .emit_esm();
    assert!(esm.contains("  const k10 = rt$keyword(null, \"m\");\n"));
    // Below `Advanced` every site constructs its literal, as before.
    for level in [OptimizationLevel::None, OptimizationLevel::Prepare] {
        let js = emit(&program, level);
        assert!(declarations(&js).is_empty(), "{level}");
        assert_eq!(js.matches("rt.keyword(null, \"a\")").count(), 4, "{level}");
    }
}

#[test]
fn advanced_output_without_literals_has_no_table() {
    let js = emit(&program("(+ 1 2)"), OptimizationLevel::Advanced);
    assert!(declarations(&js).is_empty());
}

#[test]
fn literal_symbols_share_one_value_only_at_advanced_and_keywords_at_every_level() {
    // The documented contract difference: equal constant literals may be
    // `identical?` at `Advanced`. The runtime interns literal keywords, so
    // their identity does not depend on the level.
    let source = r#"
      (defn sym [] 'x) (defn kw [] :x)
      (pr-str [(identical? (sym) 'x) (= (sym) 'x) (identical? (kw) :x)
               (identical? (kw) (kw))])"#;
    let program = program(source);
    for (level, expected) in [
        (OptimizationLevel::None, "[false true true true]\n"),
        (OptimizationLevel::Prepare, "[false true true true]\n"),
        (OptimizationLevel::Advanced, "[true true true true]\n"),
    ] {
        assert_eq!(
            execute(&format!("console.log({});", emit(&program, level))),
            expected,
            "{level}"
        );
    }
}

#[test]
fn literal_keywords_are_interned_and_keywords_built_from_data_are_fresh() {
    // Literal sites and the runtime's own constant keywords (`test` returns
    // `:no-test`) share one keyword per namespace and name at every level.
    // The `keyword` function, `read-string` and keywordized keys build fresh
    // keywords, as upstream: equal to the literal, hashing alike, found in
    // maps, sets and `case`, but not identical to it.
    every_level(
        r#"
      (defn f [] :a) (defn g [] :ns/a)
      (pr-str [(identical? (f) :a) (identical? (g) :ns/a) (identical? (f) (f))
               (identical? (test 1) :no-test)
               (= :a (keyword "a")) (identical? :a (keyword "a"))
               (identical? (keyword "a") (keyword "a")) (identical? :ns/a (keyword "ns" "a"))
               (= :ns/a (read-string ":ns/a")) (identical? :a (read-string ":a"))
               (identical? :b (first (keys (js->clj (js-obj "b" 1) :keywordize-keys true))))
               (= (hash :a) (hash (keyword "a")) (hash (read-string ":a")))
               (get {:a 1} (keyword "a")) (get {(keyword "a") 1} :a)
               (contains? #{:ns/a} (read-string ":ns/a"))
               (case (keyword "a") :a 1 2) (case (read-string ":ns/a") :ns/a 1 2)])"#,
        "[true true true true true false false false true false false true 1 1 true 1 1]",
    );
}

#[test]
fn keyword_equality_is_spelling_equality_and_literals_are_equal_only_when_identical() {
    // What a lowering of `=` on keywords may rely on (light-cljs-210): between
    // any two keywords `=` compares spellings (upstream's fqn), which the
    // `Advanced` runtime reads as `keywordFqn`; identical keywords are equal;
    // and literal keywords whose namespace and name contain no `/` are equal
    // exactly when identical. Built keywords include ones whose parts spell
    // like other parts and the positional constructor's.
    every_level(
        r#"
      (let [literals [:a :b :a/b :x.y/z :a.b :ns/a]
            built [(keyword "a") (keyword "a" "b") (keyword nil "a/b") (keyword "a/b")
                   (keyword 'x.y/z) (read-string ":a.b") (Keyword. "q" "r" "a" nil)
                   (keyword "a/b/c") (keyword "a" "b/c")]
            all (into literals built)
            spelling (fn [k] (subs (str k) 1))]
        (pr-str [(every? true? (for [x literals y literals] (= (= x y) (identical? x y))))
                 (every? true? (for [x all y all] (= (= x y) (= (spelling x) (spelling y)))))
                 (every? true? (for [x all y all :when (identical? x y)] (= x y)))
                 (every? true? (for [x all y all :when (= x y)] (= (hash x) (hash y))))]))"#,
        "[true true true false]",
    );
}

#[test]
fn keywords_that_spell_alike_with_other_parts_stay_equal_and_distinct() {
    // The structural fallback: keywords with a `/` inside a part, and the
    // positional constructor's keywords, which are fresh like `new`. They
    // equal the keyword they spell like without being identical to it (their
    // hashes may differ, as upstream), and keyword `case` matches them by
    // spelling at every level.
    every_level(
        r#"
      (let [a (keyword nil "a/b") b (keyword "a" "b") c (keyword "a/b/c")
            d (Keyword. "x" "y" "a" nil) e (Keyword. nil "a" "a" nil)]
        (pr-str [(= a b) (= a :a/b) (identical? b :a/b) (= (hash a) (hash :a/b))
                 [(namespace a) (name a) (namespace c) (name c)] (= c (keyword "a" "b/c"))
                 (= d :a) (identical? d :a) (= e :a) (identical? e :a) (= (hash e) (hash :a))
                 (case a :a/b 1 2) (case c :a 1 2) (case d :a 1 2) (case e :a 1 2)]))"#,
        r#"[true true false false [nil "a/b" nil "a"] true true false true false true 1 2 1 1]"#,
    );
}

#[test]
fn keyword_case_switches_on_the_spelling_without_printing_at_advanced() {
    let source = r#"
      (defn kind [k] (case k :a 1 :b/c 2))
      (defn missing [k] (try (kind k) (catch :default e (.-message e))))
      (pr-str [(kind :a) (kind (keyword "b" "c")) (kind (keyword nil "b/c"))
               (missing :zz) (missing :b/zz) (missing 5) (missing nil) (missing "a")])"#;
    every_level(
        source,
        r#"[1 2 2 "No matching clause: zz" "No matching clause: b/zz" "No matching clause: " "No matching clause: " "No matching clause: "]"#,
    );
    let program = program(source);
    let advanced = emit(&program, OptimizationLevel::Advanced);
    assert!(advanced.contains("switch (rt$keywordFqn(caseValue)) {"));
    assert!(advanced.contains("(rt$keywordFqn(caseValue) ?? \"\")"));
    assert!(!advanced.contains("rt$isKeyword"));
    // Below `Advanced` the output is unchanged.
    let none = emit(&program, OptimizationLevel::None);
    assert!(none.contains(
        "switch (rt.core.isKeyword(caseValue) ? rt.core.str(caseValue).slice(1) : null) {"
    ));
}

#[test]
fn literal_keyword_identity_holds_across_repl_inputs_and_redefinition() {
    // Each REPL input is its own unit over the session runtime, which interns
    // literal keywords for all of them. Read and built keywords stay fresh.
    let mut session = EvalSession::new(EvalOptions::default()).unwrap();
    let mut printed = |source: &str| {
        let value = session.eval_string(source).unwrap().value;
        let text = session.print(value).unwrap();
        session.release(value).unwrap();
        text
    };
    printed("(def k :a) (def q :ns/q) (defn f [] :a) (def m {:a 1})");
    assert_eq!(
        printed("[(identical? k (f)) (identical? k :a) (identical? q :ns/q)]"),
        "[true true true]"
    );
    printed("(defn f [] [:a :ns/q])");
    assert_eq!(
        printed(
            "[(identical? k (first (f))) (identical? q (second (f))) (identical? (first (keys m)) k)]"
        ),
        "[true true true]"
    );
    printed("(defn f [] (read-string \":a\"))");
    assert_eq!(
        printed("[(= k (f)) (identical? k (f)) (get m (f)) (hash (f)) (hash k)]"),
        printed("[true false 1 (hash :a) (hash :a)]")
    );
    // The REPL is below `Advanced`: quoted symbols are not shared.
    assert_eq!(
        printed("(defn s [] 'x) [(= (s) 'x) (identical? (s) 'x)]"),
        "[true false]"
    );
}
