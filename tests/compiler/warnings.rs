//! Public compiler diagnostics are collected separately from runtime outcomes.
use crate::temp_dir::TempDir;
use light_cljs::analyzer::{AnalysisOptions, AnalysisWarning, analyze_with_warnings};
use light_cljs::{analyze, parse};

fn warnings(source: &str, arrays: bool) -> Vec<AnalysisWarning> {
    let mut warnings = Vec::new();
    let program = analyze_with_warnings(
        source,
        &parse(source).unwrap(),
        AnalysisOptions {
            warning_mode: true,
            checked_arrays: arrays,
            ..AnalysisOptions::default()
        },
        &mut warnings,
    )
    .unwrap();
    assert!(!program.exprs.is_empty());
    for warning in &warnings {
        assert!(warning.span.start <= warning.span.end && warning.span.end <= source.len());
        assert_eq!(warning.source_index, 0);
    }
    warnings
}
fn messages(source: &str) -> Vec<String> {
    warnings(source, false)
        .into_iter()
        .map(|warning| warning.message)
        .collect()
}

#[test]
fn warning_mode_retains_strict_default_and_reports_before_later_failure() {
    let source = "(do (defn f [] missing) (quote))";
    assert!(analyze(source, &parse(source).unwrap()).is_err());
    let mut diagnostics = Vec::new();
    let error = analyze_with_warnings(
        source,
        &parse(source).unwrap(),
        AnalysisOptions {
            warning_mode: true,
            checked_arrays: false,
            ..AnalysisOptions::default()
        },
        &mut diagnostics,
    )
    .unwrap_err();
    assert!(format!("{error:?}").contains("quote"));
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].kind, "undeclared-var");
    assert_eq!(
        diagnostics[0].message,
        "Use of undeclared Var cljs.user/missing"
    );
    let source = "(fn ([x] x) ([x] x))";
    assert!(analyze(source, &parse(source).unwrap()).is_err());
    assert_eq!(warnings(source, false)[0].kind, "overload-arity");
}

#[test]
fn arithmetic_types_join_loop_recur_and_exclude_nonreturning_throw_paths() {
    assert_eq!(
        messages("(loop [x \"a\"] (if (identical? \"a\" x) (recur true) (+ 3 x)))"),
        ["cljs.core/+, all arguments must be numbers, got [number #{boolean string}] instead"]
    );
    assert!(messages("(loop [x \"a\"] (if (identical? \"a\" x) (recur 1) (+ 3 x)))").is_empty());
    assert!(messages("(+ 1 2 (try (throw (throw 2)) (catch :default _ 1)))").is_empty());
    assert_eq!(
        messages("(+ 1 2 (try (throw (throw 2)) (catch :default _ \"1\")))"),
        ["cljs.core/+, all arguments must be numbers, got [number string] instead"]
    );
    assert!(messages("(fn [x] (+ x 1))").is_empty());
}

#[test]
fn numeric_warning_order_and_unary_division_preserve_source_operations() {
    assert_eq!(
        messages("(do (+ \"a\") (- \"a\") (/ \"a\") (* \"a\"))"),
        [
            "cljs.core/+, all arguments must be numbers, got [string] instead",
            "cljs.core/-, all arguments must be numbers, got [string] instead",
            "cljs.core//, all arguments must be numbers, got [number string] instead",
            "cljs.core/*, all arguments must be numbers, got [string] instead",
        ]
    );
}

#[test]
fn checked_array_warnings_use_inferred_receivers_and_are_opt_in() {
    let source = "(and true (or (aget (js-obj \"foo\" 1) \"foo\") 2))";
    assert!(warnings(source, false).is_empty());
    let found = warnings(source, true);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].kind, "invalid-array-access");
    assert_eq!(
        found[0].message,
        "cljs.core/aget, arguments must be an array followed by numeric indices, got [object string] instead (consider goog.object/get for object access)"
    );
    assert!(
        warnings(
            "(let [^objects arr (into-array [1 2 3])] (aget arr 0))",
            true
        )
        .is_empty()
    );
    assert!(warnings("(fn [arr i] (aget arr i))", true).is_empty());
    assert_eq!(warnings("(aset (js-obj) \"a\" 2)", true).len(), 1);
}

#[test]
fn declared_arglists_and_overload_warnings_keep_definition_names() {
    assert_eq!(
        messages("(do (declare ^{:arglists '([x y])} foo) (defn foo [x]))"),
        ["cljs.user/foo declared arglists ([x y]) mismatch defined arglists ([x])"]
    );
    assert_eq!(
        messages("(defn myfun ([& x] x) ([& xs] xs))"),
        [
            "myfun: Can't have more than 1 variadic overload",
            "myfun: Can't have 2 overloads with same arity"
        ]
    );
    assert_eq!(
        warnings("(defn myfun ([x] x) ([& xs] xs))", false)[0].kind,
        "variadic-max-arity"
    );
}

#[test]
fn record_factory_and_constructor_arities_have_different_valid_signatures() {
    assert_eq!(
        messages("(do (defrecord Foo [a]) (:a (->Foo)))"),
        ["Wrong number of args (0) passed to cljs.user/->Foo"]
    );
    assert!(messages("(do (defrecord Foo [a]) (Foo. nil) (Foo. nil nil nil))").is_empty());
    assert_eq!(
        messages("(do (defrecord Foo [a]) (Foo. nil nil))"),
        ["Wrong number of args (2) passed to Foo"]
    );
    // A namespace that defines a macro drains its units at its end; the
    // record's fields are known before.
    assert_eq!(
        messages("(do (defmacro unused [] nil) (defrecord Foo [a]) (Foo. nil nil))"),
        ["Wrong number of args (2) passed to Foo"]
    );
}

#[test]
fn arity_index_keeps_metadata_overloads_variadics_and_redefinition_union() {
    let source = "(let [] (def f ^{:doc \"f\"} (fn ([x] x) ([x y & more] more))) (f) (f 1) (f 1 2) (f 1 2 3) (def f (fn [] nil)) (f))";
    assert!(
        messages(source).is_empty(),
        "all declaration arities remain visible"
    );
    assert_eq!(
        messages("(let [] (def f ^{:doc \"f\"} (fn [x y] x)) (f 1))"),
        ["Wrong number of args (1) passed to cljs.user/f"]
    );
}

#[test]
fn local_shadowing_earmuffs_and_unbound_destructuring_are_public_diagnostics() {
    assert_eq!(
        warnings("(fn [foo] (let [x js/foo] (println x)))", false)[0].kind,
        "js-shadowed-by-local"
    );
    assert!(warnings("(fn [foo] foo)", false).is_empty());
    assert_eq!(
        warnings("(def *foo* 1)", false)[0].kind,
        "non-dynamic-earmuffed-var"
    );
    assert!(warnings("(def ^:dynamic *foo* 1)", false).is_empty());
    for source in [
        "(let [{:keys! [a & :b]} {:a 1 :b 2}] b)",
        "(let [{:syms! [a & 'b]} '{a 1 b 2}] b)",
        "(let [{:strs! [a & \"b\"]} {\"a\" 1 \"b\" 2}] b)",
        "(let [{:keys [a] :or {b 2}} {:a 1}] [a b])",
    ] {
        assert_eq!(messages(source), ["Use of undeclared Var cljs.user/b"]);
    }
}

/// A macro may call a helper its namespace declares first and defines after
/// the macro's uses, as cuerdas.core's `unsafe-concat` calls its `repeat`:
/// upstream loads the `.cljc` file as Clojure before its macros expand. With
/// warnings on as off, the namespace's deferred units are analyzed once its
/// top-level forms are, and a later source failure is still reported after
/// the warnings of the forms before it.
#[test]
fn warning_mode_macros_call_later_definitions_of_their_namespace() {
    use std::fs;
    let root = TempDir::new("forward-helper");
    fs::create_dir_all(root.join("app")).unwrap();
    let header = "(ns app.text (:refer-clojure :exclude [repeat]))\n\
                  (declare repeat)\n\
                  (defmacro stars [& xs] (repeat \"*\" (count xs)))\n";
    let helper = "(defn repeat [s n] (apply str (clojure.core/repeat n s)))\n";
    let compile = |warning_mode| {
        let mut diagnostics = Vec::new();
        let js = light_cljs::compiler::compile_namespace_with_warnings(
            "app.text",
            std::slice::from_ref(root.path()),
            AnalysisOptions {
                warning_mode,
                ..AnalysisOptions::default()
            },
            &mut diagnostics,
        );
        (js, diagnostics)
    };
    fs::write(
        root.join("app/text.cljc"),
        format!(
            "{header}(defn three [] (stars 1 2 3))\n(def two (stars 1 2))\n{helper}\
             (js/console.log (three) two (repeat \"ab\" 2))"
        ),
    )
    .unwrap();
    for warning_mode in [false, true] {
        let (js, diagnostics) = compile(warning_mode);
        let js = js.unwrap_or_else(|error| panic!("warning mode {warning_mode}: {error}"));
        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
        let output =
            std::process::Command::new(std::env::var_os("NODE").unwrap_or_else(|| "node".into()))
                .args(["--eval", &js])
                .output()
                .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8(output.stdout).unwrap(), "*** ** abab\n");
    }

    // `three`'s macro cannot call `repeat` before the failure defines it, so
    // the failure is the error, after the warnings of the forms before it in
    // source order: a deferred unit's, then a top-level form's.
    fs::write(
        root.join("app/text.cljc"),
        format!(
            "{header}(defn warned [] missing)\n(js/console.log top-missing)\n\
             (defn three [] (stars 1 2 3))\n(quote)\n{helper}"
        ),
    )
    .unwrap();
    let (error, diagnostics) = compile(true);
    let error = error.unwrap_err().to_string();
    assert!(error.contains("quote"), "{error}");
    assert!(!error.contains("unavailable definition"), "{error}");
    assert_eq!(
        diagnostics
            .iter()
            .map(|w| (w.kind, w.message.as_str()))
            .collect::<Vec<_>>(),
        [
            ("undeclared-var", "Use of undeclared Var app.text/missing"),
            (
                "undeclared-var",
                "Use of undeclared Var app.text/top-missing"
            )
        ]
    );
}

/// In warning mode a namespace that defines macros drains its units at its
/// end. Its reader context still names its macros afterwards, although each
/// appended unit restores the context of its snapshot. A unit that fails
/// there ends the warnings at its own form's, as a drain after every form
/// did: a later form's warning is not reported.
#[test]
fn warning_mode_units_drained_at_the_end_keep_macros_and_stop_warnings_at_a_failure() {
    use std::fs;
    let root = TempDir::new("end-drain");
    fs::create_dir_all(root.join("app")).unwrap();
    let options = AnalysisOptions {
        warning_mode: true,
        ..AnalysisOptions::default()
    };
    fs::write(
        root.join("app/m.cljs"),
        "(ns app.m) (defmacro one [] 1) (def value (one)) (defmacro two [] 2)",
    )
    .unwrap();
    let mut diagnostics = Vec::new();
    let program = light_cljs::compiler::analyze_namespace_entry(
        "app.m",
        std::slice::from_ref(root.path()),
        &mut light_cljs::MacroHost::new(),
        options,
        &mut diagnostics,
    )
    .unwrap();
    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    assert_eq!(program.reader_contexts["app.m"].macros, ["one", "two"]);

    fs::write(
        root.join("app/m.cljs"),
        "(ns app.m) (defmacro one [] 1)\n(defn warned [] missing)\n\
         (defn bad [] (quote))\n(js/console.log later-missing)",
    )
    .unwrap();
    let mut diagnostics = Vec::new();
    let error = light_cljs::compiler::analyze_namespace_entry(
        "app.m",
        std::slice::from_ref(root.path()),
        &mut light_cljs::MacroHost::new(),
        options,
        &mut diagnostics,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("quote"), "{error}");
    assert_eq!(
        diagnostics
            .iter()
            .map(|w| (w.kind, w.message.as_str()))
            .collect::<Vec<_>>(),
        [("undeclared-var", "Use of undeclared Var app.m/missing")]
    );
}

#[test]
fn namespace_private_access_and_reserved_alias_have_structured_locations() {
    use std::fs;
    let root = TempDir::new("warnings");
    fs::create_dir_all(root.join("cljs")).unwrap();
    fs::write(
        root.join("a.cljs"),
        "(ns a) (def ^:private value 3) (defn- f [x] x)",
    )
    .unwrap();
    fs::write(
        root.join("b.cljs"),
        "(ns b (:require [a])) (a/f 1) (#'a/f 1) a/value @#'a/value",
    )
    .unwrap();
    let options = AnalysisOptions {
        warning_mode: true,
        checked_arrays: false,
        ..AnalysisOptions::default()
    };
    let mut diagnostics = Vec::new();
    light_cljs::compiler::compile_namespace_with_warnings(
        "b",
        std::slice::from_ref(root.path()),
        options,
        &mut diagnostics,
    )
    .unwrap();
    assert_eq!(
        diagnostics
            .iter()
            .map(|w| w.message.as_str())
            .collect::<Vec<_>>(),
        [
            "a is a single segment namespace",
            "b is a single segment namespace",
            "var: a/f is not public",
            "var: a/value is not public"
        ]
    );
    assert_eq!(diagnostics[0].kind, "single-segment-namespace");
    assert_eq!(diagnostics[0].source_index, 0);
    assert_eq!(diagnostics[0].span, 4..5);
    assert_eq!(diagnostics[1].kind, "single-segment-namespace");
    assert_eq!(diagnostics[1].source_index, 1);
    assert!(
        diagnostics
            .iter()
            .skip(2)
            .all(|w| w.source_index == 1 && w.kind == "private-var-access")
    );
    fs::write(root.join("cljs/js.cljs"), "(ns cljs.js)").unwrap();
    fs::write(root.join("b.cljs"), "(ns b (:require [cljs.js :as js]))").unwrap();
    assert!(light_cljs::compile_namespace("b", std::slice::from_ref(root.path())).is_err());
    diagnostics.clear();
    light_cljs::compiler::compile_namespace_with_warnings(
        "b",
        std::slice::from_ref(root.path()),
        options,
        &mut diagnostics,
    )
    .unwrap();
    assert_eq!(diagnostics.len(), 2);
    assert_eq!(diagnostics[0].kind, "single-segment-namespace");
    assert_eq!(diagnostics[0].message, "b is a single segment namespace");
    assert_eq!(diagnostics[1].kind, "js-used-as-alias");
    assert_eq!(
        diagnostics[1].message,
        "In [cljs.js :as js], the alias name js is reserved for JavaScript interop"
    );
}

#[test]
fn nested_namespace_operations_report_context_and_require_can_be_shadowed() {
    for (source, expected) in [
        (
            "(def foo (ns foo.core (:require [clojure.set :as set])))",
            "Namespace declarations must appear at the top-level.",
        ),
        (
            "(fn [] (ns nested))",
            "Namespace declarations must appear at the top-level.",
        ),
        (
            "(let [x 1] (ns* (:require 'library)))",
            "Namespace declarations must appear at the top-level.",
        ),
        (
            "(fn [] (require 'library))",
            "Calls to `require` must appear at the top-level.",
        ),
    ] {
        let error = analyze(source, &parse(source).unwrap()).unwrap_err();
        assert!(format!("{error}").contains(expected), "{source}: {error}");
    }
    assert!(messages("(let [require (fn [x] x)] (require 1))").is_empty());
}

#[test]
fn pure_function_expansion_locates_forward_reference_at_original_symbol() {
    let source = "(def f (fn [] future)) (def future 1) (f)";
    let error = analyze(source, &parse(source).unwrap()).unwrap_err();
    assert_eq!(&source[error.span], "future");
    let reports = warnings(source, false);
    let forward = reports.iter().find(|w| w.kind == "undeclared-var").unwrap();
    assert_eq!(&source[forward.span.clone()], "future");
}

#[test]
fn deferred_warning_placeholders_reuse_later_definition_identity() {
    let source = "(defn f [] future) (def future 42) (f)";
    let mut warnings = vec![];
    let program = analyze_with_warnings(
        source,
        &parse(source).unwrap(),
        AnalysisOptions {
            warning_mode: true,
            checked_arrays: false,
            ..AnalysisOptions::default()
        },
        &mut warnings,
    )
    .unwrap();
    assert_eq!(
        program
            .globals
            .iter()
            .filter(|g| g.name == "future")
            .count(),
        1
    );
    assert_eq!(
        warnings
            .iter()
            .filter(|w| w.kind == "undeclared-var")
            .count(),
        1
    );
    let evaluator = light_cljs::evaluator::Evaluator::new(Default::default()).unwrap();
    assert_eq!(
        evaluator
            .number(evaluator.evaluate(&program).unwrap())
            .unwrap(),
        42.0
    );
}

#[test]
fn namespace_arity_index_retains_overloads_redefinitions_and_namespace_identity() {
    use std::fs;
    let root = TempDir::new("namespace-arity-index");
    fs::create_dir_all(root.join("arity")).unwrap();
    fs::write(
        root.join("arity/a.cljs"),
        "(ns arity.a)
         (let [] (def f (fn [x] x)) (def f (fn [] nil)) (defrecord Pair [x y]))",
    )
    .unwrap();
    fs::write(
        root.join("arity/b.cljs"),
        "(ns arity.b (:require [arity.a :as a]))
         (let [] (defn f [x y] x)) (a/f) (a/f 1) (a/f 1 2)",
    )
    .unwrap();
    fs::write(
        root.join("arity/c.cljs"),
        "(ns arity.c (:require [arity.b :as b] [arity.a :as a]))
         (a/f) (a/f 1) (a/f 1 2) (a/->Pair 1) (b/f 1) (b/f 1 2)",
    )
    .unwrap();
    let mut diagnostics = Vec::new();
    light_cljs::compiler::compile_namespace_with_warnings(
        "arity.c",
        std::slice::from_ref(root.path()),
        AnalysisOptions {
            warning_mode: true,
            ..AnalysisOptions::default()
        },
        &mut diagnostics,
    )
    .unwrap();
    assert_eq!(
        diagnostics
            .iter()
            .map(|warning| (warning.source_index, warning.kind, warning.message.as_str()))
            .collect::<Vec<_>>(),
        [
            (
                1,
                "fn-arity",
                "Wrong number of args (2) passed to arity.a/f"
            ),
            (
                2,
                "fn-arity",
                "Wrong number of args (2) passed to arity.a/f"
            ),
            (
                2,
                "fn-arity",
                "Wrong number of args (1) passed to arity.a/->Pair"
            ),
            (
                2,
                "fn-arity",
                "Wrong number of args (1) passed to arity.b/f"
            ),
        ]
    );
}
