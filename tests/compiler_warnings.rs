//! Public compiler diagnostics are collected separately from runtime outcomes.
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

#[test]
fn namespace_private_access_and_reserved_alias_have_structured_locations() {
    use std::{
        fs,
        sync::atomic::{AtomicUsize, Ordering},
    };
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let root = std::env::temp_dir().join(format!(
        "light-warnings-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
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
    };
    let mut diagnostics = Vec::new();
    light_cljs::compiler::compile_namespace_with_warnings(
        "b",
        std::slice::from_ref(&root),
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
    assert!(light_cljs::compile_namespace("b", std::slice::from_ref(&root)).is_err());
    diagnostics.clear();
    light_cljs::compiler::compile_namespace_with_warnings(
        "b",
        std::slice::from_ref(&root),
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
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn nested_namespace_operations_report_context_and_require_can_be_shadowed() {
    for (source, expected) in [
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
        assert!(format!("{error}").contains(expected));
    }
    assert!(messages("(let [require (fn [x] x)] (require 1))").is_empty());
}
