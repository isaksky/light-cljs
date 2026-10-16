//! Loader cases are hand-authored for the bounded namespace subset unless an
//! upstream adaptation is identified beside a test. Runtime assertions execute JS.

use crate::temp_dir::TempDir;
use light_cljs::compile_namespace;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

struct Sources(TempDir);
impl Sources {
    fn new() -> Self {
        let path = TempDir::new("cljs-namespaces");
        Self(path)
    }

    fn write(&self, path: &str, source: &str) {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, source).unwrap();
    }

    fn compile(&self, entry: &str) -> Result<String, light_cljs::NamespaceCompileError> {
        compile_namespace(entry, std::slice::from_ref(self.0.path()))
    }
}

fn execute(js: &str) -> String {
    let output = Command::new(std::env::var_os("NODE").unwrap_or_else(|| "node".into()))
        .args(["--eval", js])
        .output()
        .expect("namespace tests require Node.js");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn example_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/namespaces")
}

#[test]
fn acceptance_program_prints_four_from_api_and_cli() {
    let root = example_root();
    assert_eq!(
        execute(&compile_namespace("demo.main", std::slice::from_ref(&root)).unwrap()),
        "4\n"
    );
    let output = Command::new(env!("CARGO_BIN_EXE_light-cljs"))
        .args(["--compile", "--entry", "demo.main", "--source-root"])
        .arg(root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(execute(&String::from_utf8(output.stdout).unwrap()), "4\n");
}

#[test]
fn depth_first_dependencies_and_top_level_initializers_run_once_in_source_order() {
    let sources = Sources::new();
    sources.write("app/main.cljs", "(ns app.main (:require [app.left :as l] app.right [app.shared])) (js/console.log \"main\") (js/console.log l/value app.right/value)");
    sources.write("app/left.cljs", "(ns app.left (:require [app.shared :as shared])) (def value (do (js/console.log \"left:first\") shared/value)) (js/console.log \"left:last\")");
    sources.write("app/right.cljs", "(ns app.right (:require app.shared)) (def value (do (js/console.log \"right\") app.shared/value))");
    sources.write(
        "app/shared.cljs",
        "(ns app.shared) (def value (do (js/console.log \"shared\") 7))",
    );
    assert_eq!(
        execute(&sources.compile("app.main").unwrap()),
        "shared\nleft:first\nleft:last\nright\nmain\n7 7\n"
    );
}

#[test]
fn globals_keep_namespace_identity_and_lexical_shadowing() {
    let sources = Sources::new();
    sources.write("app/math.cljs", "(ns app.math) (def answer 10) (def read-answer (fn* [] answer)) (def answer 20) (def apply-one (fn* [f] (f)))");
    sources.write("app/main.cljs", "(ns app.main (:require [app.math :as m])) (def answer 30) (js/console.log (let* [answer 40] answer) answer app.main/answer m/answer app.math/answer (m/read-answer) (m/apply-one m/read-answer))");
    assert_eq!(
        execute(&sources.compile("app.main").unwrap()),
        "40 30 30 20 20 20 20\n"
    );
}

#[test]
fn lexical_captures_and_qualified_globals_survive_shadowing_and_redefinition() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/namespaces");
    assert_eq!(
        execute(&compile_namespace("lexical.main", &[root]).unwrap()),
        "40\n30\n12\n12\n11\n12\n30\n32\n33\n"
    );
}

#[test]
fn lexical_bindings_do_not_leak_between_namespaces() {
    let sources = Sources::new();
    sources.write("scope/dep.cljs", "(ns scope.dep) (let* [secret 1] secret)");
    let source = "(ns scope.main (:require [scope.dep :as dep]))\n(let* [λ 1] secret)";
    sources.write("scope/main.cljs", source);
    let error = sources.compile("scope.main").unwrap_err();
    assert!(error.message.contains("unresolved symbol: secret"));
    assert!(error.path.unwrap().ends_with("scope/main.cljs"));
    let start = source.find("secret").unwrap();
    assert_eq!(error.span, Some(start..start + "secret".len()));
    assert_eq!(error.line_column, Some((2, 14)));
}

#[test]
fn namespace_paths_munge_hyphens_and_multiple_roots_are_explicit() {
    let entry = Sources::new();
    let deps = Sources::new();
    entry.write(
        "my_app/main.cljs",
        "(ns my-app.main (:require [lib.math-core :as m])) (js/console.log m/value)",
    );
    deps.write("lib/math_core.cljs", "(ns lib.math-core) (def value 5)");
    let roots = [
        entry.0.to_path_buf(),
        deps.0.to_path_buf(),
        entry.0.to_path_buf(),
    ];
    assert_eq!(
        execute(&compile_namespace("my-app.main", &roots).unwrap()),
        "5\n"
    );
    let output = Command::new(env!("CARGO_BIN_EXE_light-cljs"))
        .args(["--compile", "--source-root"])
        .arg(entry.0.path())
        .args(["--entry", "my-app.main", "--source-root"])
        .arg(deps.0.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(execute(&String::from_utf8(output.stdout).unwrap()), "5\n");
}

#[test]
fn distinct_sources_for_the_same_namespace_are_ambiguous() {
    let one = Sources::new();
    let two = Sources::new();
    one.write("demo.cljs", "(ns demo) 1");
    two.write("demo.cljs", "(ns demo) 2");
    let error = compile_namespace("demo", &[one.0.to_path_buf(), two.0.to_path_buf()]).unwrap_err();
    assert!(error.message.contains("ambiguous namespace demo"));
    assert!(error.message.contains(one.0.to_str().unwrap()));
    assert!(error.message.contains(two.0.to_str().unwrap()));
}

#[test]
fn missing_dependencies_report_the_requiring_file_and_span() {
    let sources = Sources::new();
    sources.write(
        "demo/main.cljs",
        "(ns demo.main\n (:require [demo.missing :as m])) 0",
    );
    let error = sources.compile("demo.main").unwrap_err();
    assert!(error.message.contains("namespace demo.missing not found"));
    assert!(error.message.contains("demo/missing.cljs"));
    assert!(error.path.unwrap().ends_with("demo/main.cljs"));
    assert_eq!(error.line_column, Some((2, 13)));
}

#[test]
fn cycles_report_the_dependency_path() {
    let sources = Sources::new();
    sources.write("a.cljs", "(ns a (:require b))");
    sources.write("b.cljs", "(ns b (:require c))");
    sources.write("c.cljs", "(ns c (:require a))");
    let error = sources.compile("a").unwrap_err();
    assert!(error.message.contains("a -> b -> c -> a"));
    assert!(error.path.unwrap().ends_with("c.cljs"));
    sources.write("a.cljs", "(ns a (:require a))");
    assert!(sources.compile("a").unwrap_err().message.contains("a -> a"));
}

#[test]
fn source_namespace_must_be_first_and_match_the_requested_name() {
    let sources = Sources::new();
    for (source, message) in [
        ("", "must begin with"),
        ("1 (ns demo)", "must begin with"),
        ("(ns different)", "does not match requested namespace demo"),
        ("(ns)", "requires a namespace name"),
        ("(ns :demo)", "namespace must use"),
    ] {
        sources.write("demo.cljs", source);
        let error = sources.compile("demo").unwrap_err();
        assert!(error.message.contains(message), "{source}: {error}");
        assert!(error.path.unwrap().ends_with("demo.cljs"));
    }
}

// Adapted validation behaviors from ClojureScript a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e,
// src/test/clojure/cljs/analyzer_tests.clj, `spec-validation`: malformed lib,
// non-symbol alias, conflicting alias, unknown clause/options. These assert our
// intentionally narrower loader diagnostics, not upstream ASTs/message wording.
#[test]
fn malformed_and_unsupported_namespace_specifications_have_explicit_diagnostics() {
    let sources = Sources::new();
    for (source, expected) in [
        ("(ns demo (:require {:foo :bar}))", "require expects"),
        ("(ns demo (:require [:foo :bar]))", "require library"),
        ("(ns demo (:require [lib :as []]))", ":as alias"),
        (
            "(ns demo (:require [lib :as x] [other :as x]))",
            "duplicate or conflicting namespace alias",
        ),
        ("(ns demo (:unless []))", "unsupported namespace clause"),
        (
            "(ns demo (:require [lib :plop true]))",
            "unsupported require options",
        ),
        (
            "(ns demo (:require [lib :as x :as y]))",
            "only one :as alias",
        ),
        ("(ns demo (:require [lib :refer :all]))", ":refer"),
        ("(ns demo (:require [lib :rename {f g}]))", ":rename"),
        // Package strings are accepted (tests/compiler/npm_packages.rs); only
        // malformed package specifications are rejected here.
        (
            "(ns demo (:require [\"\" :as p]))",
            "JavaScript package name must be a nonempty string without whitespace",
        ),
        (
            "(ns demo (:use [\"package\" :only [x]]))",
            ":use requires a namespace symbol",
        ),
        (
            "(ns demo (:require [lib :default d]))",
            ":default requires a JavaScript package string",
        ),
        ("(ns demo (:import :Thing))", "import expects"),
        ("(ns demo (:require []))", "cannot be empty"),
    ] {
        sources.write("demo.cljs", source);
        let error = sources.compile("demo").unwrap_err();
        assert!(error.message.contains(expected), "{source}: {error}");
    }
}

#[test]
fn aliases_cannot_redirect_namespace_qualifiers() {
    let sources = Sources::new();
    for (source, expected) in [
        ("(ns demo (:require [lib :as js]))", "js is reserved"),
        (
            "(ns demo (:require [lib :as demo]))",
            "conflicts with namespace qualifier demo",
        ),
        (
            "(ns demo (:require [lib :as other] other))",
            "conflicts with namespace qualifier other",
        ),
    ] {
        sources.write("demo.cljs", source);
        let error = sources.compile("demo").unwrap_err();
        assert!(error.message.contains(expected), "{source}: {error}");
    }
}

#[test]
fn analyzer_diagnostics_name_the_original_dependency_source() {
    let sources = Sources::new();
    sources.write("app/main.cljs", "(ns app.main (:require app.dep)) 0");
    sources.write("app/dep.cljs", "(ns app.dep)\n  unknown");
    let error = sources.compile("app.main").unwrap_err();
    assert!(error.path.unwrap().ends_with("app/dep.cljs"));
    assert_eq!(error.line_column, Some((2, 3)));
    assert!(error.message.contains("unresolved symbol: unknown"));
}

#[test]
fn unknown_qualifiers_are_rejected_and_loaded_namespaces_qualify() {
    let sources = Sources::new();
    // A namespace the program has loaded qualifies a symbol even when the
    // caller did not require it, as upstream resolves it: macro expansions
    // name the macro namespace's own dependencies.
    sources.write("a.cljs", "(ns a (:require b)) (js/console.log c/value)");
    sources.write("b.cljs", "(ns b (:require c))");
    sources.write("c.cljs", "(ns c) (def value 1)");
    assert_eq!(execute(&sources.compile("a").unwrap()), "1\n");
    sources.write("a.cljs", "(ns a) absent/value");
    let error = sources.compile("a").unwrap_err();
    assert!(
        error.message.contains("unknown namespace or alias absent"),
        "{error}"
    );
}

#[test]
fn parse_errors_retain_file_line_and_byte_column() {
    let sources = Sources::new();
    sources.write("demo.cljs", "(ns demo)\n(]");
    let error = sources.compile("demo").unwrap_err();
    assert!(error.path.unwrap().ends_with("demo.cljs"));
    assert_eq!(error.line_column, Some((2, 2)));
    assert!(error.message.contains("expected ), found ]"));
}

#[test]
fn lookup_requires_valid_names_and_existing_explicit_directories() {
    let sources = Sources::new();
    for invalid in [
        "../outside",
        "demo/escape",
        "demo..empty",
        "js",
        "demo.9bad",
        "demo.λ",
    ] {
        assert!(
            sources
                .compile(invalid)
                .unwrap_err()
                .message
                .contains("invalid entry namespace")
        );
    }
    assert!(
        compile_namespace("demo", &[])
            .unwrap_err()
            .message
            .contains("explicit source root")
    );
    assert!(
        compile_namespace("demo", &[sources.0.join("absent")])
            .unwrap_err()
            .message
            .contains("cannot open source root")
    );
    sources.write("file", "not a directory");
    assert!(
        compile_namespace("demo", &[sources.0.join("file")])
            .unwrap_err()
            .message
            .contains("not a directory")
    );
}

#[test]
fn namespace_cli_rejects_bad_flags_without_partial_javascript() {
    for (args, expected) in [
        (vec!["--compile", "--entry"], "--entry requires a namespace"),
        (vec!["--compile", "--entry", "demo"], "explicit source root"),
        (vec!["--compile", "--source-root", "."], "requires --entry"),
        (
            vec!["--compile", "--entry", "demo", "--entry", "other"],
            "only be specified once",
        ),
        (
            vec!["--compile", "--entry", "demo", "--source-root"],
            "requires a directory",
        ),
        (vec!["--analyze", "--entry", "demo"], "require --compile"),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_light-cljs"))
            .args(&args)
            .output()
            .unwrap();
        assert!(!output.status.success(), "{args:?}");
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(expected),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn slash_is_an_unqualified_name_and_qualified_namespace_member() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/namespaces");
    assert_eq!(
        execute(&compile_namespace("boundary.main", &[root]).unwrap()),
        "7\n7\n8\n8\n9\n"
    );
}

#[test]
fn bounded_core_is_virtual_and_does_not_override_user_namespace_members() {
    let sources = Sources::new();
    sources.write("cljs/core.cljs", "invalid this file must never be loaded");
    sources.write(
        "user/core.cljs",
        "(ns user.core) (def vector (fn* [x] x)) (def get (fn* [x] x))",
    );
    sources.write("app/main.cljs", "(ns app.main (:require [cljs.core :as c] [user.core :as u])) (def saved vector) (def vector (fn* [x] x)) (def get (fn* [x] x)) (js/console.log (vector 7) (get 8) (u/vector 9) (u/get 10) (c/get (saved 20 30) 1) (clojure.core/nth (c/vector 40) 0) (namespace ::c/x) (namespace ::u/y))");
    let js = sources.compile("app.main").unwrap();
    assert_eq!(js.matches("  const rt = ").count(), 1);
    assert_eq!(execute(&js), "7 8 9 10 30 40 cljs.core user.core\n");
}

#[test]
fn reserved_core_qualifiers_cannot_be_redirected() {
    let sources = Sources::new();
    sources.write("other.cljs", "(ns other) (def get 1)");
    for alias in ["cljs.core", "clojure.core"] {
        let source = format!("(ns app.main (:require [other :as {alias}])) 1");
        sources.write("app/main.cljs", &source);
        let error = sources.compile("app.main").unwrap_err();
        assert!(
            error.message.contains("conflicts with namespace qualifier"),
            "{error}"
        );
        assert_eq!(&source[error.span.unwrap()], alias);
    }
}

#[test]
fn shared_sources_select_cljs_before_dependencies_and_preserve_splicing_and_locations() {
    let sources = Sources::new();
    sources.write("app/main.cljs", "(ns app.main (:require [app.shared :as shared :refer [value]])) (js/console.log value shared/value)");
    sources.write("app/shared.cljc", r#"(ns ^{:author "test"} app.shared "Shared namespace" {:doc "attrs"}
      (:require #?@(:clj [[java.missing :as j]] :cljs [[app.leaf :as leaf]])))
      #?(:clj (def value (java.time.Instant/now)) :cljs (def value (+ leaf/value #?(:other 0 :default 2))))
      #?(:clj (unknown-jvm-only))
      (def data [0 #?@(:cljs [1 #?(:cljs 2)]) 3])"#);
    sources.write("app/leaf.cljc", "(ns app.leaf) (def value 5)");
    assert_eq!(execute(&sources.compile("app.main").unwrap()), "7 7\n");
    sources.write(
        "app/shared.cljc",
        "(ns app.shared)\n#?(:clj jvm-only :cljs missing)",
    );
    let error = sources.compile("app.main").unwrap_err();
    assert!(error.path.unwrap().ends_with("app/shared.cljc"));
    assert_eq!(error.line_column, Some((2, 24)));
}

#[test]
fn cljs_precedes_cljc_across_roots_but_same_extension_duplicates_remain_ambiguous() {
    let one = Sources::new();
    let two = Sources::new();
    one.write("demo.cljc", "(ns demo) (js/console.log 1)");
    two.write("demo.cljs", "(ns demo) (js/console.log 2)");
    for roots in [
        [one.0.to_path_buf(), two.0.to_path_buf()],
        [two.0.to_path_buf(), one.0.to_path_buf()],
    ] {
        assert_eq!(execute(&compile_namespace("demo", &roots).unwrap()), "2\n");
    }
    two.write("demo.cljc", "(ns demo) 3");
    assert_eq!(
        execute(&compile_namespace("demo", &[one.0.to_path_buf(), two.0.to_path_buf()]).unwrap()),
        "2\n"
    );
    fs::remove_file(two.0.join("demo.cljs")).unwrap();
    assert!(
        compile_namespace("demo", &[one.0.to_path_buf(), two.0.to_path_buf()])
            .unwrap_err()
            .message
            .contains("ambiguous")
    );
}

#[test]
fn runtime_refers_renames_and_core_exclusions_keep_qualified_identity() {
    let s = Sources::new();
    s.write(
        "library.cljs",
        "(ns library) (def value 8) (def f (fn* [x] (+ value x)))",
    );
    s.write("main.cljs", r#"(ns main (:refer-clojure :exclude [+] :rename {count size when always})
        (:require [library :as lib :refer [value f] :rename {f renamed}]))
        (def + (fn* [a b] a))
        (js/console.log (+ 5 2) (cljs.core/+ 5 2) value (renamed 3) (= renamed lib/f) (size [1 2 3]) (always true 42)
            (let* [value 9] value) library/value)"#);
    assert_eq!(
        execute(&s.compile("main").unwrap()),
        "5 7 8 11 true 3 42 9 8\n"
    );
    for expression in ["inc", "when", "+"] {
        s.write(
            "main.cljs",
            &format!("(ns main (:refer-clojure :exclude [{expression}])) {expression}"),
        );
        assert!(
            s.compile("main")
                .unwrap_err()
                .message
                .contains("unresolved symbol")
        );
    }
    s.write(
        "main.cljs",
        "(ns main (:require [library :refer [f] :rename {f renamed}])) f",
    );
    assert!(
        s.compile("main")
            .unwrap_err()
            .message
            .contains("unresolved symbol: f")
    );
}

#[test]
fn conditional_selection_validates_splices_and_does_not_read_inactive_discarded_tags() {
    let s = Sources::new();
    s.write("main.cljc", "(ns main) #?(:clj (do #_#unknown/tag 2 jvm-only)) (js/console.log #?(:default 3 :cljs 4) #?(:other 0 :cljs 7))");
    assert_eq!(execute(&s.compile("main").unwrap()), "3 7\n");
    for expression in ["[1 #?@(:cljs 2)]", "{:a #?(:clj 1)}"] {
        s.write("main.cljc", &format!("(ns main) {expression}"));
        assert!(s.compile("main").is_err());
    }
}

#[test]
fn refer_conflicts_and_nonexistent_imports_are_diagnosed_even_when_unused() {
    let s = Sources::new();
    s.write("a.cljs", "(ns a) (def value 1)");
    s.write("b.cljs", "(ns b) (def value 2)");
    for source in [
        "(ns main (:require [a :refer [missing]])) 0",
        "(ns main (:require [a :refer [value]] [b :refer [value]])) 0",
        "(ns main (:require [a :refer [value] :rename {missing renamed}])) 0",
    ] {
        s.write("main.cljs", source);
        assert!(s.compile("main").is_err(), "{source}");
    }
}

#[test]
fn qualified_definitions_syntax_quotes_and_clojure_namespace_aliases_keep_identity() {
    let s = Sources::new();
    s.write("cljs/library.cljs", "(ns cljs.library) (def value 8)");
    s.write("main.cljs", r#"(ns main (:require [clojure.library :as l :refer [value] :rename {value renamed}]) (:refer-clojure :exclude [count]))
        (def main/own 42)
        (js/console.log own main/own renamed l/value clojure.library/value cljs.library/value
            (= `renamed 'cljs.library/value) (= `count 'main/count) (= `l/value 'cljs.library/value))"#);
    assert_eq!(
        execute(&s.compile("main").unwrap()),
        "42 42 8 8 8 8 true true true\n"
    );
}

#[test]
fn shared_primitive_selection_preserves_reader_context_and_matches_cli() {
    let s = Sources::new();
    s.write("main.cljc", "(ns main) #?(:clj missing-jvm-symbol :cljs 3)");
    let shared = s.compile("main").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_light-cljs"))
        .args(["--compile", "--entry", "main", "--source-root"])
        .arg(s.0.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        shared.trim()
    );
    s.write("main.cljs", "(ns main) 3");
    assert_eq!(shared, s.compile("main").unwrap());
    assert_eq!(shared.matches("\n  const rt = ").count(), 1);
    assert_eq!(execute(&format!("console.log({shared});")), "3\n");
}

#[test]
fn recurrence_targets_and_protocol_methods_keep_namespace_identity() {
    let sources = Sources::new();
    sources.write("steps/api.cljs", "(ns steps.api) (defprotocol Step (step [self n])) (defn descend [n] (if (pos? n) (recur (dec n)) n))");
    sources.write("steps/main.cljs", "(ns steps.main (:require [steps.api :as api :refer [step]])) (let [x 17 runner (reify api/Step (step [self n] (if (pos? n) (recur self (dec n)) x)))] (js/console.log (step runner 100000) (api/descend 100000) (loop [n 2] (if (pos? n) (recur (dec n)) n))))");
    assert_eq!(execute(&sources.compile("steps.main").unwrap()), "17 0 0\n");
}

#[test]
fn namespaces_load_from_any_source_provider() {
    use light_cljs::analyzer::AnalysisOptions;
    use light_cljs::compiler::analyze_namespace_entry_with_provider;
    use light_cljs::source_provider::{LoadError, LoadRequest, SourceInput};
    use light_cljs::{ApplicationOptions, MacroHost, OptimizationLevel, prepare_application};

    // In-memory files, as the browser playground supplies them: no roots, no
    // filesystem, and a `.cljc` dependency with a macro.
    let files = [
        (
            "demo/main.cljs",
            "(ns demo.main (:require [demo.math :as m :refer [twice]])) (js/console.log (twice (m/square 3)))",
        ),
        (
            "demo/math.cljc",
            "(ns demo.math) (defmacro twice [x] `(* 2 ~x)) (defn square [x] (* x x))",
        ),
    ];
    let provider = |request: &LoadRequest| -> Result<Option<SourceInput>, LoadError> {
        let relative = String::from_utf16_lossy(request.namespace.name()).replace('.', "/");
        Ok(files
            .iter()
            .find(|(path, _)| {
                path.strip_suffix(".cljs")
                    .or_else(|| path.strip_suffix(".cljc"))
                    .is_some_and(|stem| stem == relative)
            })
            .map(|(path, source)| SourceInput::new(*path, *source)))
    };
    let compile = |entry: &str| {
        analyze_namespace_entry_with_provider(
            entry,
            &provider,
            &mut MacroHost::new(),
            AnalysisOptions::default(),
            &mut Vec::new(),
        )
    };
    let program = compile("demo.main").unwrap();
    let js = prepare_application(
        &program,
        ApplicationOptions::new(OptimizationLevel::Prepare),
    )
    .emit_js();
    assert_eq!(execute(&js), "18\n");

    let error = compile("demo.missing").unwrap_err();
    assert!(
        error.message.contains("namespace demo.missing not found"),
        "{error}"
    );
}
