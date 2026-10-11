//! Namespace operations execute source programs; reflection returns actual Vars.
use light_cljs::{MacroHost, compile_namespace, compile_namespace_with_macros};
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
            "light-cljs-namespace-completion-{}-{}",
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
    fn run(&self, entry: &str) {
        let js = compile_namespace(entry, std::slice::from_ref(&self.0)).unwrap();
        Self::check(&js);
    }
    fn check(js: &str) {
        let output = Command::new("node")
            .args(["-e", &format!("console.log({js});")])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8(output.stdout).unwrap(), "true\n");
    }
}
impl Drop for Sources {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn ordinary_top_level_requires_import_values_macros_and_shared_alias_metadata() {
    let s = Sources::new();
    s.write(
        "library.cljs",
        "(ns library) (def value 41) (defn add [x] (+ x value))",
    );
    s.write(
        "macros.cljs",
        r#"(ns macros)
         (defmacro twice [x] (list '+ x x))
         (defmacro import-state []
           (list 'quote [(:requires (:ns &env))
                         (:uses (:ns &env))
                         (:require-macros (:ns &env))
                         (:use-macros (:ns &env))]))"#,
    );
    s.write(
        "main.cljs",
        r#"(ns main)
      (require '[library :as l :refer [add] :rename {add plus}])
      (do (require '[library :as l]) (require '[library :as l]) (require 'library))
      (require '[macros :as m :refer [twice]])
      (require '[macros :as m :refer [twice]])
      (refer-clojure :exclude '[map] :rename '{when always})
      (defn map [] :own)
      (and (= 42 (plus 1)) (= l/value library/value) (= 6 (m/twice 3))
           (= 8 (twice 4)) (= :own (map)) (= 9 (always true 9))
           (= [{'library 'library 'l 'library 'macros 'macros 'm 'macros}
               {'twice 'macros 'always 'cljs.core} {} {}]
              (m/import-state)))"#,
    );
    s.run("main");
    s.write("main.cljs", "(ns main) (use '[library :only [add]]) (use '[macros :only [twice]]) (= 43 (add (twice 1)))");
    s.run("main");
}

#[test]
fn host_globals_and_imports_keep_constructor_function_identity() {
    let s = Sources::new();
    s.write(
        "main.cljs",
        r#"(ns main (:require-global [React :as react :refer [createElement]]))
      (refer-global :only '[Date])
      (import '[Host Widget])
      (set! js/React (js-obj "createElement" (fn [x] (str "element:" x))))
      (set! js/Host (js-obj "Widget" (js* "(function Widget(x) { this.x = x; })")))
      (and (identical? Date js/Date) (identical? React react)
           (identical? createElement react/createElement)
           (= "element:a" (createElement "a"))
           (= 7 (.-x (Widget. 7)))
           (= ['Widget Widget] (find (ns-imports 'main) 'Widget)))"#,
    );
    s.run("main");
}

#[test]
fn namespace_reflection_uses_real_vars_and_includes_macro_generated_definitions() {
    let s = Sources::new();
    s.write(
        "macros.cljs",
        "(ns macros) (defmacro definitions [] '(do (def generated 7) (def ^:private hidden 8)))",
    );
    s.write("library.cljs", "(ns library (:require [macros :refer [definitions]])) (defn visible [] 42) (defn- private-fn [] 1) (definitions)");
    s.write(
        "main.cljs",
        r#"(ns main (:require [library :as l]))
      (let [publics (ns-publics 'library) interns (ns-interns 'library)]
        (and (= ['visible #'library/visible] (find publics 'visible))
             (= 42 ((get publics 'visible)))
             (= 7 @(get publics 'generated))
             (not (contains? publics 'hidden)) (not (contains? publics 'private-fn))
             (contains? interns 'hidden) (contains? interns 'private-fn)
             (= {} (ns-publics 'unknown.namespace))))"#,
    );
    s.run("main");
}

#[test]
fn bare_imports_are_ignored_and_reflection_does_not_leak_across_compilations() {
    let s = Sources::new();
    let mut host = MacroHost::new();
    s.write("main.cljs", "(ns main (:import goog)) (def old 1) (and (= {} (ns-imports 'main)) (contains? (ns-publics 'main) 'old))");
    Sources::check(
        &compile_namespace_with_macros("main", std::slice::from_ref(&s.0), &mut host).unwrap(),
    );
    s.write(
        "main.cljs",
        "(ns main) (and (= {} (ns-imports 'main)) (not (contains? (ns-publics 'main) 'old)))",
    );
    Sources::check(
        &compile_namespace_with_macros("main", std::slice::from_ref(&s.0), &mut host).unwrap(),
    );
}

#[test]
fn namespace_operation_errors_are_located_and_nested_requires_stay_invalid() {
    let s = Sources::new();
    s.write("library.cljs", "(ns library) (def value 1)");
    s.write("other.cljs", "(ns other) (def value 2)");
    for (body, expected) in [
        ("(require [library :as l])", "must be quoted"),
        ("(require library)", "must be quoted"),
        (
            "(do (require '[library :as l]) (require '[other :as l]))",
            "conflicting namespace alias",
        ),
        ("(fn [] (require '[library :as l]))", "require"),
        ("(import '[:invalid Thing])", "import namespace"),
        ("(ns-publics main)", "quoted symbol"),
    ] {
        s.write("main.cljs", &format!("(ns main)\n{body}"));
        let error = compile_namespace("main", std::slice::from_ref(&s.0)).unwrap_err();
        assert!(error.message.contains(expected), "{body}: {error}");
        assert_eq!(error.line_column.unwrap().0, 2);
    }
}

#[test]
fn closure_host_namespaces_resolve_aliases_refers_and_dotted_paths() {
    let s = Sources::new();
    s.write(
        "main.cljs",
        r#"(ns main (:require [goog.example :as g :refer [value]])
      (:import [goog.example Widget]))
      (set! js/goog (js-obj "example" (js-obj "value" 42 "Widget" (js* "(function Widget(){})"))))
      (and (= 42 value g/value g.value goog.example.value)
           (identical? Widget goog.example.Widget))"#,
    );
    s.run("main");
    // Explicit ClojureScript source wins over the external Closure convention.
    s.write("goog/example.cljs", "(ns goog.example) (def value 7)");
    s.write(
        "main.cljs",
        "(ns main (:require [goog.example :as g])) (= 7 g/value)",
    );
    s.run("main");
}

#[test]
fn reserved_js_alias_warns_only_under_the_explicit_warning_api() {
    use light_cljs::analyzer::AnalysisOptions;
    use light_cljs::compiler::compile_namespace_with_warnings;
    let s = Sources::new();
    s.write("cljs/js.cljs", "(ns cljs.js)");
    s.write("main.cljs", "(ns main (:require [cljs.js :as js])) true");
    assert!(
        compile_namespace("main", std::slice::from_ref(&s.0))
            .unwrap_err()
            .message
            .contains("js is reserved")
    );
    let mut warnings = Vec::new();
    let js = compile_namespace_with_warnings(
        "main",
        std::slice::from_ref(&s.0),
        AnalysisOptions {
            warning_mode: true,
            checked_arrays: false,
            ..AnalysisOptions::default()
        },
        &mut warnings,
    )
    .unwrap();
    Sources::check(&js);
    assert_eq!(warnings.len(), 2);
    assert_eq!(warnings[0].kind, "single-segment-namespace");
    assert_eq!(warnings[0].message, "main is a single segment namespace");
    assert_eq!(warnings[0].span, 4..8);
    assert_eq!(warnings[1].kind, "js-used-as-alias");
    assert_eq!(
        warnings[1].message,
        "In [cljs.js :as js], the alias name js is reserved for JavaScript interop"
    );
    assert!(warnings.iter().all(|warning| warning.source_index == 1));
}

#[test]
fn repeated_header_aliases_preserve_function_identity_and_macro_invocation() {
    use light_cljs::analyzer::AnalysisOptions;
    use light_cljs::compiler::compile_namespace_with_warnings;
    let s = Sources::new();
    s.write(
        "boundary/lib.cljs",
        "(ns boundary.lib) (defn add [n] (+ n 41))",
    );
    s.write(
        "boundary/macros.cljs",
        "(ns boundary.macros) (defmacro twice [x] (list '+ x x))",
    );
    for (entry, file, expected_warnings) in [
        ("boundary", "boundary.cljs", 1),
        ("boundary.main", "boundary/main.cljs", 0),
    ] {
        let source = format!(
            "(ns {entry} (:require [boundary.lib :as x] [boundary.lib :as x]
                              [boundary.macros :as m :refer [twice]]
                              [boundary.macros :as m :refer [twice]]))
             (and (identical? x/add boundary.lib/add) (= 42 (x/add 1))
                  (= 6 (m/twice 3)) (= 8 (twice 4)))"
        );
        s.write(file, &source);
        s.run(entry);
        let mut warnings = Vec::new();
        let js = compile_namespace_with_warnings(
            entry,
            std::slice::from_ref(&s.0),
            AnalysisOptions {
                warning_mode: true,
                checked_arrays: false,
                ..AnalysisOptions::default()
            },
            &mut warnings,
        )
        .unwrap();
        Sources::check(&js);
        assert_eq!(warnings.len(), expected_warnings);
        if let Some(warning) = warnings.first() {
            assert_eq!(warning.kind, "single-segment-namespace");
            assert_eq!(warning.message, "boundary is a single segment namespace");
            assert_eq!(&source[warning.span.clone()], "boundary");
            // Both the value and macro namespace are compiled source dependencies.
            assert_eq!(warning.source_index, 2);
        }
    }
}

#[test]
fn ordinary_alias_conflicts_for_value_and_macro_namespaces_fail_in_both_modes() {
    use light_cljs::analyzer::AnalysisOptions;
    use light_cljs::compiler::compile_namespace_with_warnings;
    let s = Sources::new();
    for declaration in ["(def value 1)", "(defmacro value [] 1)"] {
        s.write(
            "boundary/lib.cljs",
            &format!("(ns boundary.lib) {declaration}"),
        );
        s.write(
            "boundary/other.cljs",
            &format!("(ns boundary.other) {declaration}"),
        );
        s.write(
            "boundary/main.cljs",
            "(ns boundary.main (:require [boundary.lib :as x] [boundary.other :as x]))",
        );
        for warning_mode in [false, true] {
            let error = compile_namespace_with_warnings(
                "boundary.main",
                std::slice::from_ref(&s.0),
                AnalysisOptions {
                    warning_mode,
                    checked_arrays: false,
                    ..AnalysisOptions::default()
                },
                &mut Vec::new(),
            )
            .unwrap_err();
            assert!(
                error.message.contains("conflicting namespace alias: x"),
                "{error}"
            );
        }
    }
}

#[test]
fn retired_macro_import_syntax_reports_located_migration_diagnostics_in_both_modes() {
    use light_cljs::analyzer::AnalysisOptions;
    use light_cljs::compiler::compile_namespace_with_warnings;
    let s = Sources::new();
    // These are deliberate local rejection probes. Pinned CLJS oracle sources
    // under compiler-namespaces retain their original supported dialect.
    for body in [
        "(ns boundary.main (:require-macros [missing.macros :as m]))",
        "(ns boundary.main (:use-macros [missing.macros :only [twice]]))",
        "(ns boundary.main (:require [missing.macros :refer-macros [twice]]))",
        "(ns boundary.main (:require [missing.macros :include-macros true]))",
        "(ns boundary.main) (require-macros '[missing.macros :as m])",
        "(ns boundary.main) (use-macros '[missing.macros :only [twice]])",
    ] {
        s.write("boundary/main.cljs", &format!("\n{body}"));
        for warning_mode in [false, true] {
            let error = compile_namespace_with_warnings(
                "boundary.main",
                std::slice::from_ref(&s.0),
                AnalysisOptions {
                    warning_mode,
                    checked_arrays: false,
                    ..AnalysisOptions::default()
                },
                &mut Vec::new(),
            )
            .unwrap_err();
            assert!(
                error
                    .message
                    .contains("retired; import macros and functions with ordinary :require"),
                "{body}: {error}"
            );
            assert!(
                error.message.contains(".cljs/.cljc sources"),
                "{body}: {error}"
            );
            assert_eq!(error.line_column.unwrap().0, 2, "{body}: {error}");
            assert!(error.path.unwrap().ends_with("boundary/main.cljs"));
        }
    }
}
