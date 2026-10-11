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
                    ..AnalysisOptions::default()
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
            ..AnalysisOptions::default()
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
// The old runtime-defmacro ABI is retired by compiled bootstrap. Source macros
// are now compiler declarations; this intentionally differs from the old oracle.
#[test]
fn inline_defmacro_is_a_compile_time_function_with_hidden_form_and_environment() {
    run(
        r#"(do (defmacro sample [x] (list 'quote [(:context &env) &form x]))
      (= [:expr '(sample 9) 9] (sample 9)))"#,
        false,
    );
    assert!(
        compile("(defmacro sample [] 1) sample")
            .unwrap_err()
            .to_string()
            .contains("compile-time macro")
    );
}
#[test]
fn inline_defmacro_multiple_and_variadic_methods_execute_during_compilation() {
    run(
        r#"(do (defmacro methods ([x] x) ([x y & more] (last more)))
      (= [1 4] [(methods 1) (methods 1 2 3 4)]))"#,
        false,
    );
    assert!(
        compile("(defmacro methods [x] x) (apply methods [1])")
            .unwrap_err()
            .to_string()
            .contains("compile-time macro")
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
            ..AnalysisOptions::default()
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
            ..AnalysisOptions::default()
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
fn inline_macro_arity_errors_are_compile_time_diagnostics() {
    let error = compile("(defmacro fixed ([x] x) ([x y] y)) (fixed)")
        .unwrap_err()
        .to_string();
    assert!(error.contains("arity"), "{error}");
    assert!(error.contains("definition"), "{error}");
}
