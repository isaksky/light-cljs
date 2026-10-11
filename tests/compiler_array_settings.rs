//! File-scoped compiler state is separate from the public runtime Var.
use light_cljs::analyzer::{AnalysisOptions, CheckedArrays, analyze_with_macro_warnings};
use light_cljs::evaluator::{Evaluator, EvaluatorOptions, RUNTIME_SOURCE};
use light_cljs::{MacroHost, Program, emitter::emit_js_unit, parse};
use std::{path::PathBuf, process::Command};

fn program(
    source: &str,
    checks: CheckedArrays,
) -> (Program, Vec<light_cljs::analyzer::AnalysisWarning>) {
    let mut warnings = Vec::new();
    let p = analyze_with_macro_warnings(
        source,
        &parse(source).unwrap(),
        &mut MacroHost::new(),
        AnalysisOptions {
            array_checks: checks,
            ..Default::default()
        },
        &mut warnings,
    )
    .unwrap();
    (p, warnings)
}
fn both(p: &Program, expected: &str) {
    let e = Evaluator::new(EvaluatorOptions::default()).unwrap();
    assert_eq!(
        e.print(e.evaluate(p).unwrap()).unwrap(),
        expected,
        "QuickJS"
    );
    let script = format!(
        "const rt = {RUNTIME_SOURCE}; console.log(rt.core.prStr(({})(rt)));",
        emit_js_unit(p)
    );
    let out = Command::new("node").args(["-e", &script]).output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8(out.stdout).unwrap().trim(),
        expected,
        "Node"
    );
}

#[test]
fn array_setting_var_and_macro_state_execute_in_both_phases() {
    let source = r#"(do
      (defmacro state [] (:light-cljs/unchecked-arrays &env))
      (def before (state))
      (set! *unchecked-arrays* true)
      (def during (state))
      (defn captured [] (state))
      (set! *unchecked-arrays* false)
      (def after (state))
      (set! cljs.core/*unchecked-arrays* true)
      (def qualified [(state) *unchecked-arrays*])
      (set! cljs.core/*unchecked-arrays* false)
      (def effects (atom []))
      (def computed (set! *unchecked-arrays* (do (swap! effects conj :rhs) true)))
      (defmacro runtime-root [] (list 'quote [*unchecked-arrays* @#'*unchecked-arrays*]))
      (and (= [false true false true false] [before during after (captured) (state)])
           (= [false true] qualified)
           (= [false false] (runtime-root))
           (false? *unchecked-arrays*) (false? cljs.core/*unchecked-arrays*)
           (false? @#'*unchecked-arrays*) (var? #'*unchecked-arrays*)
           (= '*unchecked-arrays* (:name (meta #'*unchecked-arrays*)))
           (= 'cljs.core (:ns (meta #'*unchecked-arrays*)))
           (= "cljs/core.cljs" (:file (meta #'*unchecked-arrays*)))
           (= 30 (:line (meta #'*unchecked-arrays*)))
           (nil? computed) (empty? @effects)))"#;
    let (p, warnings) = program(source, CheckedArrays::Off);
    assert!(warnings.is_empty(), "{warnings:?}");
    both(&p, "true");
    // Fresh compilations and reuse of the same macro host both start false.
    let mut host = MacroHost::new();
    for source in [
        "(set! *unchecked-arrays* true)",
        "(defmacro state [] (:light-cljs/unchecked-arrays &env)) (false? (state))",
    ] {
        let p =
            light_cljs::analyze_with_macros(source, &parse(source).unwrap(), &mut host).unwrap();
        both(
            &p,
            if source.starts_with("(set!") {
                "nil"
            } else {
                "true"
            },
        );
    }
}

#[test]
fn named_function_macro_environments_capture_boolean_array_settings() {
    // Reader declarations are a finite source-ordered snapshot, distinct from
    // the argument/self locals and captured alongside the array setting.
    let source = r#"(do
      (defmacro inspect [self expected-bindings]
        (let [locals (:locals &env)
              reader (:light-cljs/reader-context &env)]
          (list 'quote
            [(:light-cljs/unchecked-arrays &env)
             (boolean? (:light-cljs/unchecked-arrays &env))
             (= #{:locals :context :ns :line :column
                  :light-cljs/macros :light-cljs/unchecked-arrays
                  :light-cljs/reader-context}
                (set (keys &env)))
             (= (if self (conj '#{x y} self) '#{x y}) (set (keys locals)))
             (= :return (:context &env))
             (= {:name 'x :op :binding :local :arg :arg-id 0 :binding-form? true}
                (select-keys (get locals 'x) [:name :op :local :arg-id :binding-form?]))
             (= {:name 'y :op :binding :local :arg :arg-id 1 :binding-form? true}
                (select-keys (get locals 'y) [:name :op :local :arg-id :binding-form?]))
             (if self
               (= {:name self :op :binding :local :fn}
                  (select-keys (get locals self) [:name :op :local :arg-id :binding-form?]))
               true)
             (= #{:refers :excludes :bindings} (set (keys reader)))
             (= [] (:refers reader))
             (= [] (:excludes reader))
             (and (vector? (:bindings reader))
                  (every? string? (:bindings reader))
                  (= (count expected-bindings) (count (:bindings reader)))
                  (= expected-bindings (set (:bindings reader))))
             (and (= 'cljs.user (:name (:ns &env)))
                  (= :cljs.user/probe (read-string "::probe"))
                  (= 'cljs.core/map (second (read-string "`map"))))])))
      (defn named-default [x y] (inspect nil #{"inspect" "named-default"}))
      (def self-default (fn foo [x y]
        (inspect foo #{"inspect" "named-default" "self-default"})))
      (set! *unchecked-arrays* true)
      (defn named-enabled [x y]
        (inspect nil #{"inspect" "named-default" "self-default" "named-enabled"}))
      (def self-enabled (fn foo [x y]
        (inspect foo #{"inspect" "named-default" "self-default" "named-enabled" "self-enabled"})))
      (set! *unchecked-arrays* false)
      (defn named-reset [x y]
        (inspect nil #{"inspect" "named-default" "self-default" "named-enabled" "self-enabled" "named-reset"}))
      (def self-reset (fn foo [x y]
        (inspect foo #{"inspect" "named-default" "self-default" "named-enabled" "self-enabled" "named-reset" "self-reset"})))
      [(named-default 1 2) (self-default 1 2)
       (named-enabled 1 2) (self-enabled 1 2)
       (named-reset 1 2) (self-reset 1 2)])"#;
    let (p, warnings) = program(source, CheckedArrays::Off);
    assert!(warnings.is_empty(), "{warnings:?}");
    both(
        &p,
        "[[false true true true true true true true true true true true true] [false true true true true true true true true true true true true] [true true true true true true true true true true true true true] [true true true true true true true true true true true true true] [false true true true true true true true true true true true true] [false true true true true true true true true true true true true]]",
    );
}

#[test]
fn checked_array_options_follow_source_order_and_preserve_argument_effects() {
    let source = r#"(do (def effects (atom []))
      (def errors (atom []))
      (set! *print-err-fn* (fn [e] (swap! errors conj :warning)))
      (defn probe [tag]
        (try (aget (do (swap! effects conj tag) #js [7]) 2)
          (catch js/Error e :error)))
      (set! *unchecked-arrays* true)
      (def unchecked (aget (do (swap! effects conj :unchecked) #js [7]) 2))
      (set! *unchecked-arrays* false)
      (def checked (probe :checked))
      (def target #js [1])
      (def written (try (aset (do (swap! effects conj :target) target)
                        (do (swap! effects conj :index) 2)
                        (do (swap! effects conj :value) 9))
                        (catch js/Error e :error)))
      [(nil? unchecked) checked written @effects @errors (alength target)])"#;
    for (mode, expected) in [
        (
            CheckedArrays::Off,
            "[true nil 9 [:unchecked :checked :target :index :value] [] 3]",
        ),
        (
            CheckedArrays::Warn,
            "[true nil 9 [:unchecked :checked :target :index :value] [:warning :warning] 3]",
        ),
        (
            CheckedArrays::Error,
            "[true :error :error [:unchecked :checked :target :index :value] [] 1]",
        ),
    ] {
        both(&program(source, mode).0, expected);
    }
    let source = "(do (aget #js {} \"a\") (set! *unchecked-arrays* true) (aget #js {} \"b\") (set! *unchecked-arrays* false) (aset #js {} \"c\" 1))";
    let (_, warnings) = program(source, CheckedArrays::Warn);
    assert_eq!(warnings.len(), 2, "{warnings:?}");
    assert!(warnings[0].message.contains("cljs.core/aget"));
    assert!(warnings[1].message.contains("cljs.core/aset"));
    for (warning, text) in warnings
        .iter()
        .zip(["(aget #js {} \"a\")", "(aset #js {} \"c\" 1)"])
    {
        assert_eq!(&source[warning.span.clone()], text);
    }
    // Disabling assertions preserves direct access in either checked mode.
    for mode in [CheckedArrays::Warn, CheckedArrays::Error] {
        both(
            &program("(binding [*assert* false] (nil? (aget #js [] 2)))", mode).0,
            "true",
        );
    }
    // Analyze even a no-op RHS, so unresolved names never vanish.
    assert!(
        light_cljs::compile("(set! *unchecked-arrays* missing)")
            .unwrap_err()
            .to_string()
            .contains("missing")
    );
}

#[test]
fn complete_source_bodies_retain_modules_and_file_isolation() {
    for name in ["cljs-2861-test", "unchecked-arrays-file-scope-test"] {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/compiler-array-settings")
            .join(name);
        let js = light_cljs::compile_namespace("cljs.array-access-test", &[root]).unwrap();
        let source = format!(
            "JSON.stringify(({js})(() => {{ throw new Error('unexpected host request'); }}))"
        );
        let e = Evaluator::new(EvaluatorOptions::default()).unwrap();
        assert_eq!(
            e.string(e.evaluate_js(&source).unwrap()).unwrap(),
            "true",
            "{name} QuickJS"
        );
        let out = Command::new("node")
            .args(["-e", &format!("console.log({source});")])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(
            String::from_utf8(out.stdout).unwrap().trim(),
            "true",
            "{name} Node"
        );
    }
}

#[test]
fn checked_options_reset_between_loaded_files_and_capture_definitions() {
    let root =
        std::env::temp_dir().join(format!("light-cljs-array-settings-{}", std::process::id()));
    std::fs::create_dir_all(root.join("scope")).unwrap();
    std::fs::write(
        root.join("scope/beta.cljs"),
        "(ns scope.beta) (set! *unchecked-arrays* true) (defn read [] (aget #js [] 2))",
    )
    .unwrap();
    std::fs::write(root.join("scope/alpha.cljs"), "(ns scope.alpha (:require [scope.beta :as beta])) (defn run [] (and (nil? (beta/read)) (= :error (try (aget #js [] 2) (catch js/Error e :error))))) run").unwrap();
    let mut warnings = Vec::new();
    let js = light_cljs::compiler::compile_namespace_with_warnings(
        "scope.alpha",
        &[root.clone()],
        AnalysisOptions {
            array_checks: CheckedArrays::Error,
            ..Default::default()
        },
        &mut warnings,
    )
    .unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let script = format!("JSON.stringify(({js})())");
    let e = Evaluator::new(EvaluatorOptions::default()).unwrap();
    assert_eq!(e.string(e.evaluate_js(&script).unwrap()).unwrap(), "true");
    let out = Command::new("node")
        .args(["-e", &format!("console.log({script});")])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8(out.stdout).unwrap().trim(), "true");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn cli_array_check_modes_execute_the_compiled_program() {
    for (flag, source) in [
        ("--checked-arrays-warn", "(nil? (aget #js [] 2))"),
        (
            "--checked-arrays-error",
            "(try (aget #js [] 2) false (catch js/Error e true))",
        ),
        (
            "--checked-arrays-error",
            "(do (set! *unchecked-arrays* true) (nil? (aget #js [] 2)))",
        ),
    ] {
        let compiled = Command::new(env!("CARGO_BIN_EXE_light-cljs"))
            .args([flag, "--compile", "--text", source])
            .output()
            .unwrap();
        assert!(
            compiled.status.success(),
            "{}",
            String::from_utf8_lossy(&compiled.stderr)
        );
        let script = format!(
            "console.log({});",
            String::from_utf8(compiled.stdout).unwrap()
        );
        let out = Command::new("node").args(["-e", &script]).output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(String::from_utf8(out.stdout).unwrap().trim(), "true");
    }
}

#[test]
fn array_source_provenance_and_oracle_routing_are_reproducible() {
    for path in [
        "tests/reference/compiler-array-settings.py",
        "tests/reference/compiler-array-settings-report-tests.py",
    ] {
        let result = Command::new("python3")
            .args(["-B", path])
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
    }
}
