//! Public spellings accepted by both pinned compilers, plus selected warning APIs.
use light_cljs::analyzer::{AnalysisOptions, analyze_with_warnings};
use light_cljs::{compile, emit_js, parse};
use std::process::Command;
fn run(source: &str, warning_mode: bool) {
    let js = if warning_mode {
        emit_js(
            &analyze_with_warnings(
                source,
                &parse(source).unwrap(),
                AnalysisOptions {
                    warning_mode,
                    checked_arrays: false,
                },
                &mut Vec::new(),
            )
            .unwrap(),
        )
    } else {
        compile(source).unwrap()
    };
    let result = Command::new("node")
        .args(["-e", &format!("console.log({js});")])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(String::from_utf8(result.stdout).unwrap(), "true\n");
}
#[test]
fn unqualified_host_paths_keep_receiver_and_lexical_shadowing() {
    run(
        r#"(do (set! js/hostSample (js-obj "value" 7 "method" (js* "(function(x){return this.value+x;})")))
      (and (= 9 (hostSample.method 2))
           (js/isNaN (let [hostSample (js-obj "value" 10 "method" (js* "(function(x){return this.value+x;})"))] (hostSample.method 2)))))"#,
        false,
    );
}
#[test]
fn dotted_def_uses_host_assignment_and_amp_is_a_valid_global_name() {
    run(
        r#"(do (set! js/hostDefs (js-obj)) (def hostDefs.answer 40) (def & 2)
      (set! hostDefs.answer (+ hostDefs.answer &))
      (and (= 42 hostDefs.answer js/hostDefs.answer @#'hostDefs.answer)
           (= 2 & @#'&) (= 'answer (:name (meta #'hostDefs.answer))) (= 'hostDefs (:ns (meta #'hostDefs.answer)))))"#,
        false,
    );
}
#[test]
fn double_dots_munge_before_host_path_splitting_and_qualified_members_keep_receiver() {
    run(
        r#"(do (set! js/Math_DOT__DOT_sqrt 17)
      (set! js/hostNested (js-obj "a" (js-obj "value" 18 "b" (js* "(function(x){return this.value+x;})"))
        "_a" (js-obj "base" 19 "value" (js* "(function(){return this.base;})"))))
      (and (= 17 js/Math..sqrt) (= 20 (.a/b js/hostNested 2)) (= 18 (.-value (.-a js/hostNested))) (= 19 (.-a/value js/hostNested))))"#,
        false,
    );
}
#[test]
fn unknown_namespace_warning_mode_keeps_actual_host_namespace_values() {
    let source = "(do (set! js/absent (js-obj \"value\" 19)) (= 19 absent/value))";
    assert!(compile(source).is_err());
    let mut warnings = Vec::new();
    analyze_with_warnings(
        source,
        &parse(source).unwrap(),
        AnalysisOptions {
            warning_mode: true,
            checked_arrays: false,
        },
        &mut warnings,
    )
    .unwrap();
    assert_eq!(
        warnings.iter().map(|w| w.kind).collect::<Vec<_>>(),
        ["undeclared-ns", "undeclared-var"]
    );
    run(source, true);
}
#[test]
fn inline_defmacro_is_a_runtime_function_with_hidden_args_metadata_and_macro_marker() {
    run(
        r#"(do (defmacro sample "sample documentation" {:custom 7} [x] [&form &env x])
      (and (= [:form {:locals 1} 9] (sample :form {:locals 1} 9))
           (true? (.-cljs$lang$macro sample)) (:macro (meta #'sample))
           (= 7 (:custom (meta #'sample))) (= "sample documentation" (:doc (meta #'sample)))
           (= '([x]) (:arglists (meta #'sample)))))"#,
        false,
    );
}
#[test]
fn inline_defmacro_multiple_and_variadic_methods_apply_and_receiver_are_real() {
    run(
        r#"(do (defmacro boundaryMethods ([x] [&form &env x]) ([x y & more] [&form &env x y more]))
      (defmacro receiver [x] (this-as self [self &form &env x]))
      (let [host (js-obj) values (.call receiver host :f :e 7)]
        (and (= [:f :e 1] (boundaryMethods :f :e 1))
             (= [:f :e 1 2 '(3 4)] (apply boundaryMethods :f :e [1 2 3 4]))
             (identical? host (first values)) (= [:f :e 7] (vec (rest values))))))"#,
        false,
    );
}
#[test]
fn anonymous_raw_overloads_match_ordered_public_warning_messages() {
    let source = "(fn* ([a b] a) ([a & xs] a))";
    let mut warnings = Vec::new();
    analyze_with_warnings(
        source,
        &parse(source).unwrap(),
        AnalysisOptions {
            warning_mode: true,
            checked_arrays: false,
        },
        &mut warnings,
    )
    .unwrap();
    assert_eq!(
        warnings
            .iter()
            .map(|w| w.message.as_str())
            .collect::<Vec<_>>(),
        [
            ": Can't have fixed arity function with more params than variadic function",
            ": Can't have 2 overloads with same arity"
        ]
    );
}

#[test]
fn dotted_host_var_metadata_reports_both_test_reference_resolutions() {
    let source = "(do (def hostDefs.answer 1) (var hostDefs.answer))";
    let mut warnings = Vec::new();
    analyze_with_warnings(
        source,
        &parse(source).unwrap(),
        AnalysisOptions {
            warning_mode: true,
            checked_arrays: false,
        },
        &mut warnings,
    )
    .unwrap();
    assert_eq!(
        warnings
            .iter()
            .map(|warning| (warning.kind, warning.message.as_str()))
            .collect::<Vec<_>>(),
        [
            (
                "undeclared-ns",
                "No such namespace: hostDefs, could not locate hostDefs.cljs, hostDefs.cljc, or JavaScript source providing \"hostDefs\""
            ),
            ("undeclared-var", "Use of undeclared Var hostDefs/answer"),
            (
                "undeclared-ns",
                "No such namespace: hostDefs, could not locate hostDefs.cljs, hostDefs.cljc, or JavaScript source providing \"hostDefs\""
            ),
            ("undeclared-var", "Use of undeclared Var hostDefs/answer"),
        ]
    );
}

#[test]
fn inline_macro_dispatch_exposes_actual_js_abi_and_user_arity_errors() {
    run(
        r#"(do
      (defmacro fixedMacro ([x] x) ([x y] y))
      (defmacro variMacro [x & xs] xs)
      (defmacro mixedMacro ([x] x) ([x y & xs] xs))
      (and (= "Invalid arity: 0" (try (fixedMacro :f :e) (catch :default e (.-message e))))
           (= "Invalid arity: -2" (try (apply fixedMacro []) (catch :default e (.-message e))))
           (= [1 4 1 3 1 4] [(.-length fixedMacro) (.-cljs$lang$maxFixedArity fixedMacro)
                              (.-length variMacro) (.-cljs$lang$maxFixedArity variMacro)
                              (.-length mixedMacro) (.-cljs$lang$maxFixedArity mixedMacro)])
           (= 4 (.-length (.-cljs$core$IFn$_invoke$arity$4 fixedMacro)))
           (= 1 (fixedMacro :f :e 1)) (= 2 (apply fixedMacro [:f :e 1 2]))
           (= '(2 3) (apply variMacro [:f :e 1 2 3]))
           (= '([x] [x y & xs]) (:arglists (meta #'mixedMacro)))))"#,
        false,
    );
}
