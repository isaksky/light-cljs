//! Loader cases are hand-authored for the bounded namespace subset unless an
//! upstream adaptation is identified beside a test. Runtime assertions execute JS.

use light_cljs::compile_namespace;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT_DIR: AtomicUsize = AtomicUsize::new(0);

struct Sources(PathBuf);
impl Sources {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "light-cljs-namespaces-{}-{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn write(&self, path: &str, source: &str) {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, source).unwrap();
    }

    fn compile(&self, entry: &str) -> Result<String, light_cljs::NamespaceCompileError> {
        compile_namespace(entry, std::slice::from_ref(&self.0))
    }
}

impl Drop for Sources {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
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
fn namespace_paths_munge_hyphens_and_multiple_roots_are_explicit() {
    let entry = Sources::new();
    let deps = Sources::new();
    entry.write(
        "my_app/main.cljs",
        "(ns my-app.main (:require [lib.math-core :as m])) (js/console.log m/value)",
    );
    deps.write("lib/math_core.cljs", "(ns lib.math-core) (def value 5)");
    let roots = [entry.0.clone(), deps.0.clone(), entry.0.clone()];
    assert_eq!(
        execute(&compile_namespace("my-app.main", &roots).unwrap()),
        "5\n"
    );
    let output = Command::new(env!("CARGO_BIN_EXE_light-cljs"))
        .args(["--compile", "--source-root"])
        .arg(&entry.0)
        .args(["--entry", "my-app.main", "--source-root"])
        .arg(&deps.0)
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
    let error = compile_namespace("demo", &[one.0.clone(), two.0.clone()]).unwrap_err();
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
        ("(ns demo (:require [lib :refer [f]]))", ":refer"),
        ("(ns demo (:require [lib :rename {f g}]))", ":rename"),
        (
            "(ns demo (:require-macros [lib :as m]))",
            "unsupported namespace clause",
        ),
        (
            "(ns demo (:require [\"package\" :as p]))",
            "JavaScript package imports are unsupported",
        ),
        (
            "(ns demo (:require \"package\"))",
            "JavaScript package imports are unsupported",
        ),
        ("(ns demo (:import Thing))", "unsupported namespace clause"),
        ("(ns demo (:require []))", "cannot be empty"),
    ] {
        sources.write("demo.cljs", source);
        let error = sources.compile("demo").unwrap_err();
        assert!(error.message.contains(expected), "{source}: {error}");
    }
}

#[test]
fn aliases_cannot_duplicate_or_redirect_namespace_qualifiers() {
    let sources = Sources::new();
    for (source, expected) in [
        (
            "(ns demo (:require [lib :as x] [lib :as x]))",
            "duplicate or conflicting",
        ),
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
fn unknown_and_transitive_only_qualifiers_are_rejected() {
    let sources = Sources::new();
    sources.write("a.cljs", "(ns a (:require b)) c/value");
    sources.write("b.cljs", "(ns b (:require c))");
    sources.write("c.cljs", "(ns c) (def value 1)");
    let error = sources.compile("a").unwrap_err();
    assert!(
        error.message.contains("unknown namespace or alias c"),
        "{error}"
    );
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
