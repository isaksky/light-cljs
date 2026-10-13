//! Compiler macro integration through compiler-owned source and JavaScript execution.
//! CLJS environment/resolution observations
//! are documented in docs/compiler.md; tests need only Rust/Node.
use light_cljs::{
    MacroHost, analyze_with_macros, compile, compile_namespace, compile_namespace_with_macros,
    emit_js, parse,
};
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
            "light-cljs-macros-{}-{}",
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
    fn compile(&self, entry: &str) -> Result<String, light_cljs::NamespaceCompileError> {
        compile_namespace(entry, std::slice::from_ref(&self.0))
    }
    fn compile_host(
        &self,
        entry: &str,
        host: &mut MacroHost,
    ) -> Result<String, light_cljs::NamespaceCompileError> {
        compile_namespace_with_macros(entry, std::slice::from_ref(&self.0), host)
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
        .expect("compiler integration requires Node");
    assert!(
        output.status.success(),
        "{}\n{js}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}
fn result(js: &str) -> String {
    execute(&format!("console.log({js});"))
}
// Historical fixtures use real ordinary imports so macro definitions and helpers
// run in the same compiler-owned JavaScript sessions as user source namespaces.
struct SourceMacros {
    sources: Sources,
    host: MacroHost,
    header: String,
}
impl std::ops::Deref for SourceMacros {
    type Target = MacroHost;
    fn deref(&self) -> &MacroHost {
        &self.host
    }
}
impl std::ops::DerefMut for SourceMacros {
    fn deref_mut(&mut self) -> &mut MacroHost {
        &mut self.host
    }
}
trait CompileMacroFixture {
    fn compile(&mut self, source: &str) -> Result<String, light_cljs::CompileError>;
}
impl CompileMacroFixture for MacroHost {
    fn compile(&mut self, source: &str) -> Result<String, light_cljs::CompileError> {
        light_cljs::compile_with_macros(source, self)
    }
}
impl CompileMacroFixture for SourceMacros {
    fn compile(&mut self, source: &str) -> Result<String, light_cljs::CompileError> {
        self.sources
            .write("cljs/user.cljs", &format!("{}{source}", self.header));
        self.sources
            .compile_host("cljs.user", &mut self.host)
            .map_err(|error| {
                let span = error.span.clone().unwrap_or(0..source.len());
                let span = if error
                    .path
                    .as_ref()
                    .is_some_and(|p| p.ends_with("cljs/user.cljs"))
                {
                    span.start.saturating_sub(self.header.len())
                        ..span.end.saturating_sub(self.header.len())
                } else {
                    0..source.len()
                };
                light_cljs::CompileError::Analysis(light_cljs::AnalysisError {
                    kind: light_cljs::analyzer::AnalysisErrorKind::MacroExpansion(
                        error.to_string(),
                    ),
                    span,
                })
            })
    }
}
fn compile_with_macros(
    source: &str,
    host: &mut impl CompileMacroFixture,
) -> Result<String, light_cljs::CompileError> {
    host.compile(source)
}
fn host(source: &str, refers: &[&str]) -> SourceMacros {
    let sources = Sources::new();
    sources.write("macros.cljs", source);
    let host = MacroHost::new();
    SourceMacros {
        sources,
        host,
        header: format!(
            "(ns cljs.user (:require [macros :as m :refer [{}]])) ",
            refers.join(" ")
        ),
    }
}

#[test]
fn two_runtime_namespaces_use_cljs_macros_through_api_and_cli() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/macros");
    assert_eq!(
        execute(&compile_namespace("demo.main", std::slice::from_ref(&root)).unwrap()),
        "12\n"
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
    assert_eq!(execute(&String::from_utf8(output.stdout).unwrap()), "12\n");
}

#[test]
fn ordinary_core_macros_expand_inside_lexical_environments() {
    let source = r#"
        (defn choose "doc" {:private true} [x]
          (let [x (if x 4 5)] (when x (-> x (js/Math.imul 2)))))
        (def make (fn [x] (fn [] (or nil x))))
        (js/console.log (choose false) ((make 7)) (cond false 0 true 9) (and true 12))
    "#;
    assert_eq!(execute(&compile(source).unwrap()), "10 7 9 12\n");
    assert_eq!(
        result(&compile("(cljs.core/let [x 4] ((clojure.core/fn [y] y) x))").unwrap()),
        "4\n"
    );
}

#[test]
fn locals_shadow_imported_macros_and_qualified_calls_keep_namespace_identity() {
    let mut h = host("(ns macros) (defmacro value [] 7)", &["value"]);
    let source = r#"(def runtime-value 3)
        (js/console.log runtime-value (value) (m/value)
          (let [value (fn [] 9)] (value))
          (let [value (fn [] 9)] (macros/value)))"#;
    assert_eq!(
        execute(&compile_with_macros(source, &mut h).unwrap()),
        "3 7 7 9 7\n"
    );
    // A source declaration cannot switch binding kind. A local runtime def
    // shadows an ordinary referred macro while qualified calls still expand.
    let conflict = compile("(defmacro value [] 7) (def value 3)").unwrap_err();
    assert!(
        conflict.to_string().contains("distinct names"),
        "{conflict}"
    );
    assert_eq!(
        execute(
            &compile_with_macros(
                "(def value (fn [] 3)) (js/console.log (value) (m/value))",
                &mut h
            )
            .unwrap()
        ),
        "3 7\n"
    );
    assert_eq!(
        result(&compile("(def when (fn* [x] x)) (when 8)").unwrap()),
        "8\n"
    );
    assert_eq!(
        result(&compile("(let* [let (fn* [x] x)] (let 6))").unwrap()),
        "6\n"
    );
}

#[test]
fn ordinary_macro_imports_also_expose_runtime_bindings() {
    let s = Sources::new();
    s.write(
        "api.cljs",
        "(ns api) (def runtime-only 40) (defmacro value [] 7)",
    );
    s.write("runtime.cljs", "(ns runtime) (def value 5)");
    s.write(
        "main.cljs",
        "(ns main (:require [runtime :as r] [api :as a])) (js/console.log r/value (a/value))",
    );
    assert_eq!(execute(&s.compile("main").unwrap()), "5 7\n");
    s.write(
        "main.cljs",
        "(ns main (:require [api :as a])) a/runtime-only",
    );
    assert_eq!(result(&s.compile("main").unwrap()), "40\n");
    s.write(
        "main.cljs",
        "(ns main (:require [runtime :as a])) (a/value)",
    );
    // This resolves an ordinary runtime call; macro api/value was not imported.
    assert!(s.compile("main").unwrap().contains(".call(null"));
    let mut h = host(
        "(ns macros) (def hidden 33) (defmacro read-hidden [] hidden)",
        &["read-hidden"],
    );
    assert_eq!(
        result(&compile_with_macros("(read-hidden)", &mut h).unwrap()),
        "33\n"
    );
    assert_eq!(
        result(&compile_with_macros("macros/hidden", &mut h).unwrap()),
        "33\n"
    );
    assert!(compile_with_macros("m/read-hidden", &mut h).is_err());
}

#[test]
fn cljs_env_has_binding_records_context_namespace_and_unevaluated_form() {
    let mut h = host(
        r#"(ns macros)
      (defmacro inspect-env []
        (str (:context &env) "/" (:name (:ns &env)) "/"
          (:op (get (:locals &env) 'x)) "/" (:local (get (:locals &env) 'x)) "/"
          (:arg-id (get (:locals &env) 'x)) "/" (contains? (:locals &env) 'self)))
      (defmacro syntax [x] (str (first &form) "/" x "/" (:line &env)))
    "#,
        &["inspect-env", "syntax"],
    );
    assert_eq!(
        result(&compile_with_macros("(let [x (inspect-env)] x)", &mut h).unwrap()),
        ":expr/cljs.user////false\n"
    );
    assert_eq!(
        result(&compile_with_macros("(let [x 1] (inspect-env))", &mut h).unwrap()),
        ":return/cljs.user/:binding/:let//false\n"
    );
    assert_eq!(
        result(&compile_with_macros("((fn self [x] (inspect-env)) 1)", &mut h).unwrap()),
        ":return/cljs.user/:binding/:arg/0/true\n"
    );
    assert_eq!(
        result(&compile_with_macros("(syntax (undefined never-evaluated))", &mut h).unwrap()),
        "syntax/(undefined never-evaluated)/1\n"
    );
    let discarded = "(js/console.log (let [x 1] (js/console.log (inspect-env)) (inspect-env)))";
    assert_eq!(
        execute(&compile_with_macros(discarded, &mut h).unwrap()),
        ":expr/cljs.user/:binding/:let//false\n:return/cljs.user/:binding/:let//false\n"
    );
}

#[test]
fn macro_namespaces_select_cljs_and_refresh_on_each_compilation() {
    let s = Sources::new();
    s.write(
        "macros.cljc",
        "(ns macros) #?(:clj (defmacro value [] 1) :cljs (defmacro value [] 9))",
    );
    s.write(
        "main.cljs",
        "(ns main (:require [macros :refer [value]])) (js/console.log (value))",
    );
    let mut h = MacroHost::new();
    assert_eq!(execute(&s.compile_host("main", &mut h).unwrap()), "9\n");
    s.write("macros.cljc", "(ns macros) (defmacro value [] 2)");
    assert_eq!(execute(&s.compile_host("main", &mut h).unwrap()), "2\n");
    assert_eq!(execute(&s.compile("main").unwrap()), "2\n");
    let mut isolated = MacroHost::new();
    assert!(compile_with_macros("(macros/value)", &mut isolated).is_err());
}

#[test]
fn macro_code_uses_cljs_dependencies_and_caller_aliases_without_allowing_cljs_conditionals() {
    let s = Sources::new();
    s.write("helper.cljs", "(ns helper) (defn select [x] x)");
    s.write(
        "macros.cljs",
        "(ns macros (:require [helper :as h])) (defmacro identity-form [x] (h/select x))",
    );
    s.write("main.cljs", "(ns main (:require [macros :refer [identity-form]])) (js/console.log (identity-form #?(:cljs 4 :clj 8)))");
    let error = s.compile("main").unwrap_err();
    assert!(
        error.message.contains("conditional") || error.message.contains("ReaderConditional"),
        "{error}"
    );
    s.write(
        "macros.cljs",
        "(ns macros (:require [helper :as h])) (defmacro keyword-name [x] (h/select (str x)))",
    );
    s.write("runtime.cljs", "(ns runtime)");
    s.write("main.cljs", "(ns main (:require [runtime :as r] [macros :as m :refer [keyword-name]])) (js/console.log (keyword-name ::m/a) (keyword-name ::r/b) (keyword-name ::c))");
    assert_eq!(
        execute(&s.compile("main").unwrap()),
        ":macros/a :runtime/b :main/c\n"
    );
}

#[test]
fn returned_forms_keep_utf16_negative_zero_and_number_boundaries() {
    let mut h = host(
        r#"(ns macros) (defmacro text [] "\uD800")
        (defmacro negzero [] -0.0) (defmacro too-big [] 9007199254740992)
        (defmacro inner [n] 4) (defmacro outer [] '(inner 9007199254740992))"#,
        &["text", "negzero", "too-big", "inner", "outer"],
    );
    let js = compile_with_macros(
        "(js/console.log (.charCodeAt (text) 0) (js/Object.is (negzero) -0.0) (outer))",
        &mut h,
    )
    .unwrap();
    assert_eq!(execute(&js), "55296 true 4\n");
    assert_eq!(
        result(&compile_with_macros("(too-big)", &mut h).unwrap()),
        "9007199254740992\n"
    );
}

#[test]
fn generated_collections_remain_macro_data_until_used_as_runtime_expressions() {
    let mut h = host(
        "(ns macros) (defmacro inner [x] (if (= (:v x) :yes) 4 8)) (defmacro outer [] '(inner {:v :yes}))",
        &["inner", "outer"],
    );
    assert_eq!(
        result(&compile_with_macros("(outer)", &mut h).unwrap()),
        "4\n"
    );
}

#[test]
fn generated_diagnostics_point_to_call_sites_and_explain_unsupported_runtime() {
    let mut h = host(
        "(ns macros) (defmacro bad [] '(loop* [x 1] (inc (recur 2)))) (defmacro bad-character [] \\a) (defmacro explode [] (throw (ex-info \"broken\" {})))",
        &["bad", "bad-character", "explode"],
    );
    for (call, message) in [("(bad)", "generated by bad"), ("(explode)", "broken")] {
        let source = format!("; λ\n{call}");
        let error = compile_with_macros(&source, &mut h).unwrap_err();
        assert!(error.to_string().contains(message), "{error}");
        let light_cljs::CompileError::Analysis(error) = error else {
            panic!()
        };
        assert_eq!(error.span, 5..source.len());
    }
    let source = "((fn [x] {:pre [x]} x) 7)";
    assert_eq!(result(&compile(source).unwrap()), "7\n");
}

#[test]
fn macro_import_validation_and_loader_failures_are_located() {
    let s = Sources::new();
    s.write(
        "macros.cljs",
        "(ns macros) (def value 1) (defmacro hidden {:private true} [] 4) (defmacro visible [] 5)",
    );
    for (header, message) in [
        (
            "(:require [macros :refer [missing]])",
            "public Var or macro",
        ),
        ("(:require [macros :refer [hidden]])", "private"),
        (
            "(:require [macros :refer [visible visible]])",
            "duplicate or conflicting refer",
        ),
        ("(:require [missing :as m])", "not found"),
        (
            "(:require [macros :refer :all])",
            "sequence of simple symbols",
        ),
    ] {
        s.write("main.cljs", &format!("(ns main {header}) 1"));
        let error = s.compile("main").unwrap_err();
        assert!(error.message.contains(message), "{error}");
        assert!(error.path.unwrap().ends_with("main.cljs"));
        assert!(error.span.is_some());
    }
    s.write(
        "main.cljs",
        "(ns main (:require [macros :as m])) (m/visible)",
    );
    s.write("macros.cljc", "(ns macros) (defmacro visible [] 5)");
    assert_eq!(result(&s.compile("main").unwrap()), "5\n");
}

#[test]
fn compiled_source_macros_preserve_form_env_and_caller_owned_syntax() {
    let source = r#"
      (defmacro answer [x]
        (assert (= 'answer (first &form)))
        (assert (= 2 (count &form)))
        (assert (= :binding (get-in &env [:locals x :op])))
        x)
      (let [x 17] (answer x))"#;
    let syntax = parse(source).unwrap();
    let original = syntax.clone();
    let mut host = MacroHost::new();
    let program = analyze_with_macros(source, &syntax, &mut host).unwrap();
    assert_eq!(
        syntax, original,
        "analysis must not mutate caller-owned syntax"
    );
    assert_eq!(result(&emit_js(&program)), "17\n");
}

#[test]
fn recursive_and_growing_expansions_obey_depth_count_size_and_execution_limits() {
    let mut h = host(
        "(ns macros) (defmacro again [] '(do (again))) (defmacro one [] 1) (defmacro grow [] '(do 1 2 3 4)) (defmacro spin [] (loop [] (recur)))",
        &["again", "one", "grow", "spin"],
    );
    h.limits.expansion_depth = 8;
    assert!(
        compile_with_macros("(again)", &mut h)
            .unwrap_err()
            .to_string()
            .contains("depth limit")
    );
    h.limits.expansions = 2;
    assert!(
        compile_with_macros("(do (one) (one) (one))", &mut h)
            .unwrap_err()
            .to_string()
            .contains("expansion limit")
    );
    h.limits.generated_nodes = 3;
    assert!(
        compile_with_macros("(grow)", &mut h)
            .unwrap_err()
            .to_string()
            .contains("generated-node limit")
    );
    h.limits.generated_nodes = 100_000;
    h.limits.expansions = 1024;
    h.evaluator_options.poll_budget = 10_000;
    assert!(
        compile_with_macros("(spin)", &mut h)
            .unwrap_err()
            .to_string()
            .contains("budget")
    );
    h.evaluator_options = Default::default();
    // Per-compilation counters are fresh after failure; no poisoned expansion cache.
    assert_eq!(
        result(&compile_with_macros("(one)", &mut h).unwrap()),
        "1\n"
    );
}

#[test]
fn macro_call_context_matches_cljs_and_recovers_after_expansion_failure() {
    let mut h = host(
        "(ns macros) (defmacro context [] (str (:context &env))) (defmacro fail [] (throw (js/Error. \"context-failure\")))",
        &["context", "fail"],
    );
    assert_eq!(
        execute(
            &compile_with_macros("(js/console.log (do 1 (context)) (do (context)))", &mut h)
                .unwrap()
        ),
        ":return :expr\n"
    );
    assert!(
        compile_with_macros("(fail)", &mut h)
            .unwrap_err()
            .to_string()
            .contains("context-failure")
    );
    assert_eq!(
        result(&compile_with_macros("(context)", &mut h).unwrap()),
        ":return\n"
    );
}

#[test]
fn clj_companions_are_never_compiler_sources() {
    let s = Sources::new();
    s.write("same.cljs", "(ns same) (let [x 1] x)");
    s.write("same.clj", "(ns same) (defmacro value [] 42)");
    s.write(
        "main.cljs",
        "(ns main (:require [same :refer [value]])) (value)",
    );
    let mut h = MacroHost::new();
    s.compile_host("same", &mut h).unwrap();
    assert!(
        s.compile_host("main", &mut h)
            .unwrap_err()
            .message
            .contains("public Var or macro")
    );
}

#[test]
fn macro_namespace_effects_are_shared_once_per_compilation_with_fresh_reused_hosts() {
    let s = Sources::new();
    s.write(
        "macros.cljs",
        "(ns macros) (def counter (atom 0)) (defmacro next-value [] (swap! counter inc))",
    );
    s.write(
        "dep.cljs",
        "(ns dep (:require [macros :refer [next-value]])) (def value (next-value))",
    );
    s.write("main.cljs", "(ns main (:require [dep :as d] [macros :refer [next-value]])) (js/console.log d/value (next-value))");
    assert_eq!(execute(&s.compile("main").unwrap()), "1 2\n");
    assert_eq!(execute(&s.compile("main").unwrap()), "1 2\n");
    let mut h = MacroHost::new();
    assert_eq!(execute(&s.compile_host("main", &mut h).unwrap()), "1 2\n");
    assert_eq!(execute(&s.compile_host("main", &mut h).unwrap()), "1 2\n");
}

#[test]
fn macro_import_effects_follow_runtime_dependency_analysis_order() {
    let s = Sources::new();
    s.write(
        "shared.cljs",
        "(ns shared) (def state (atom 0)) (defmacro value [] @state)",
    );
    s.write(
        "b.cljs",
        "(ns b (:require [shared :refer [value]])) (def x (value))",
    );
    s.write(
        "modify.cljs",
        "(ns modify (:require [shared :as s])) (reset! s/state 9)",
    );
    s.write(
        "a.cljs",
        "(ns a (:require [b :as b] modify)) (js/console.log b/x)",
    );
    assert_eq!(execute(&s.compile("a").unwrap()), "0\n");
}

#[test]
fn default_interpreter_user_identity_does_not_hide_user_macro_source() {
    let s = Sources::new();
    s.write("user.cljs", "(ns user) (defmacro value [] 42)");
    s.write(
        "main.cljs",
        "(ns main (:require [user :refer [value]])) (value)",
    );
    assert_eq!(result(&s.compile("main").unwrap()), "42\n");
}

#[test]
fn exhausted_expansion_depth_does_not_run_macro_effects() {
    let mut h = host(
        "(ns macros) (def counter (atom 0)) (defmacro effect [] (swap! counter inc))",
        &["effect"],
    );
    h.limits.expansion_depth = 0;
    assert!(
        compile_with_macros("(effect)", &mut h)
            .unwrap_err()
            .to_string()
            .contains("depth limit")
    );
    // The compiler rejects expansion before executing its producer, even if
    // running that producer would throw a distinctive error.
    h.sources.write(
        "macros.cljs",
        "(ns macros) (defmacro effect [] (throw (js/Error. \"effect-ran\")))",
    );
    let error = compile_with_macros("(effect)", &mut h)
        .unwrap_err()
        .to_string();
    assert!(error.contains("depth limit"), "{error}");
    assert!(!error.contains("effect-ran"), "{error}");
    h.limits.expansion_depth = 64;
    assert!(
        compile_with_macros("(effect)", &mut h)
            .unwrap_err()
            .to_string()
            .contains("effect-ran")
    );
}

#[test]
fn generated_vectors_and_keyword_names_execute_with_owned_semantic_values() {
    let mut h = host(
        "(ns macros) (defmacro data [] [:made/by-macro :other/x]) (defmacro key [] (keyword \"made\" \"answer\"))",
        &["data", "key"],
    );
    let source = "(js/console.log (count (data)) (name (key)) (namespace (key)) (name (nth (data) 1)) (= (key) :made/answer))";
    assert_eq!(
        execute(&compile_with_macros(source, &mut h).unwrap()),
        "2 answer made x true\n"
    );

    let mut h = host(
        r#"(ns macros) (defmacro surrogate [] (keyword "\uD800" "\uD800"))"#,
        &["surrogate"],
    );
    let source = "(let [k (surrogate)] (js/console.log (.charCodeAt (name k) 0) (.charCodeAt (namespace k) 0)))";
    assert_eq!(
        execute(&compile_with_macros(source, &mut h).unwrap()),
        "55296 55296\n"
    );
}

#[test]
fn direct_auto_keywords_use_the_same_ordinary_aliases_as_macro_input() {
    let mut h = host(
        "(ns macros) (defmacro identity-form [x] x)",
        &["identity-form"],
    );
    assert_eq!(
        result(&compile_with_macros("(= ::m/x (identity-form ::m/x))", &mut h).unwrap()),
        "true\n"
    );
    assert_eq!(
        result(&compile_with_macros("(namespace ::m/x)", &mut h).unwrap()),
        "macros\n"
    );
    let sources = Sources::new();
    sources.write("runtime.cljs", "(ns runtime)");
    sources.write("macros.cljs", "(ns macros) (defmacro identity-form [x] x)");
    sources.write("main.cljs", "(ns main (:require [runtime :as r] [macros :as m])) (js/console.log (namespace ::m/x) (namespace (m/identity-form ::m/x)))");
    assert_eq!(
        execute(&sources.compile("main").unwrap()),
        "macros macros\n"
    );
}

#[test]
fn quoted_macro_values_preserve_symbols_effects_and_generated_provenance() {
    let mut h = host(
        "(ns macros) (def counter (atom 0)) (defmacro danger [] (swap! counter inc) 99) (defmacro data [] '(quote [unresolved-name (danger) a/x])) (defmacro invalid [] '(quote [ok \\a])) (defmacro untouched [] (assert (= 0 @counter)) true)",
        &["danger", "data", "invalid", "untouched"],
    );
    let source = "(js/console.log (name (nth (data) 0)) (name (first (nth (data) 1))) (= (nth (data) 2) 'a/x) (= '(danger) (list 'danger)) (untouched))";
    assert_eq!(
        execute(&compile_with_macros(source, &mut h).unwrap()),
        "unresolved-name danger true true true\n"
    );
    assert_eq!(
        result(&compile_with_macros("(nth (invalid) 1)", &mut h).unwrap()),
        "a\n"
    );
    assert_eq!(
        result(&compile("(let [[x & xs] [1 2 3]] (= [x xs] [1 '(2 3)]))").unwrap()),
        "true\n"
    );
}

#[test]
fn quoted_generated_symbols_preserve_lone_surrogates_but_executable_names_reject_them() {
    let mut h = host(
        r#"(ns macros)
      (defmacro quoted [] (list 'quote (symbol "\uD800" "\uDC00")))
      (defmacro executable [] (symbol "\uD800" "\uDC00"))"#,
        &["quoted", "executable"],
    );
    let source = "(let [s (quoted)] (js/console.log (.charCodeAt (name s) 0) (.charCodeAt (namespace s) 0)))";
    assert_eq!(
        execute(&compile_with_macros(source, &mut h).unwrap()),
        "56320 55296\n"
    );
    let error = compile_with_macros("(executable)", &mut h).unwrap_err();
    assert!(error.to_string().contains("non-Unicode executable symbol"));
}

#[test]
fn generated_maps_preserve_owned_children_order_keys_and_quoted_data() {
    let mut h = host(
        "(ns macros) (defmacro identity-form [x] x) (defmacro data [] {:a [1 2]}) (defmacro quoted [] '(quote {danger (unresolved 1)}))",
        &["identity-form", "data", "quoted"],
    );
    let entries = (0..16)
        .map(|i| format!("(f {}) (f {})", i * 2, i * 2 + 1))
        .collect::<Vec<_>>()
        .join(" ");
    let source = format!(
        "(let [f (fn* [x] (js/console.log x) x) m (identity-form {{{entries}}})] (js/console.log (count m) (= (:a (data)) [1 2]) (= (get (quoted) 'danger) '(unresolved 1))))"
    );
    // Both independent references evaluate reader-ordered keys, then values.
    // The exact original source and historical insertion-order expectation are
    // retained in literal-map-traversal-historical-macro-receipt.json.
    let expected = [
        14, 16, 12, 30, 2, 20, 18, 24, 10, 26, 0, 22, 8, 6, 28, 4, 15, 17, 13, 31, 3, 21, 19, 25,
        11, 27, 1, 23, 9, 7, 29, 5,
    ]
    .into_iter()
    .map(|i| format!("{i}\n"))
    .collect::<String>()
        + "16 true true\n";
    assert_eq!(
        execute(&compile_with_macros(&source, &mut h).unwrap()),
        expected
    );
    let source = "(let* [f (fn* [x] (js/console.log x) x) a 1 b 1] (js/console.log (get (identity-form {a (f 2) b (f 3)}) 1)))";
    assert_eq!(
        execute(&compile_with_macros(source, &mut h).unwrap()),
        "2\n3\n3\n"
    );
    let source = "; λ\n(identity-form {:a missing})";
    let error = compile_with_macros(source, &mut h).unwrap_err();
    assert!(
        error.to_string().contains("unresolved symbol: missing"),
        "{error}"
    );
    let light_cljs::CompileError::Analysis(error) = error else {
        panic!()
    };
    assert_eq!(&source[error.span], "missing");
}

#[test]
fn generated_map_keys_keep_reader_name_parts_before_runtime_spelling_equality() {
    let mut h = host(
        r#"(ns macros)
      (defmacro map-names [] {(keyword nil "a/b") 1 (keyword "a" "b") 2})
      (defmacro key-parts [] [(keyword nil "a/b") (keyword "a" "b")])"#,
        &["map-names", "key-parts"],
    );
    let source = "(let [[a b] (key-parts)] (js/console.log (count (map-names)) (namespace a) (name a) (namespace b) (name b)))";
    assert_eq!(
        execute(&compile_with_macros(source, &mut h).unwrap()),
        "1 null a/b a b\n"
    );
    let evaluator = light_cljs::evaluator::Evaluator::new(Default::default()).unwrap();
    // The exact direct-source fixtures also pass through ordinary source macro
    // imports. Generated keys keep both sets of parts; the generated literal
    // map keeps the first key representative and the last value.
    for row in include_str!("fixtures/compiler-keyword-map-keys.tsv")
        .lines()
        .filter(|row| !row.is_empty() && !row.starts_with('#'))
    {
        let columns: Vec<_> = row.split('\t').collect();
        let source = columns[1]
            .replace(
                "a (keyword nil \"a/b\") b (keyword \"a\" \"b\")",
                "[a b] (key-parts)",
            )
            .replace("m {a 1 b 2}", "m (map-names)");
        let js =
            compile_with_macros(&format!("(js/console.log (pr-str {source}))"), &mut h).unwrap();
        let expected = format!("{}\n", columns[2]);
        assert_eq!(execute(&js), expected, "Node: {}", columns[0]);
        let value = evaluator
            .evaluate_js(&format!(
                "(() => {{ let output = ''; globalThis.console = {{log: value => {{ output += value + '\\n'; }}}}; try {{ {js}; return output; }} finally {{ delete globalThis.console; }} }})()"
            ))
            .unwrap();
        assert_eq!(
            evaluator.string(value).unwrap(),
            expected,
            "QuickJS: {}",
            columns[0]
        );
    }
}

#[test]
fn generated_sets_preserve_members_source_effects_and_quoted_data() {
    let mut h = host(
        "(ns macros) (defmacro identity-form [x] x) (defmacro data [] #{:a [1 2]}) (defmacro quoted [] '(quote #{danger (unresolved 1)})) (defmacro ignore [x] nil)",
        &["identity-form", "data", "quoted", "ignore"],
    );
    let members = (0..16)
        .map(|i| format!("(f {i})"))
        .collect::<Vec<_>>()
        .join(" ");
    let source = format!(
        "(let [f (fn* [x] (js/console.log x) x) s (identity-form #{{{members}}})] (js/console.log (count s) (contains? (data) [1 2]) (contains? (quoted) '(unresolved 1))))"
    );
    // The original source operands remain unchanged. Both pinned references
    // traverse their reader set in this order before evaluating each once;
    // literal-set-traversal-contracts.json retains the independent witness.
    let expected = [14, 3, 12, 9, 2, 15, 5, 10, 7, 0, 13, 8, 1, 6, 4, 11]
        .into_iter()
        .map(|i| format!("{i}\n"))
        .collect::<String>()
        + "16 true true\n";
    assert_eq!(
        execute(&compile_with_macros(&source, &mut h).unwrap()),
        expected
    );
    // Distinct readable forms execute in order, then the pinned checkout
    // rejects equal resulting members. Maven 1.12.134 instead deduplicates.
    let source = "(try (let* [f (fn* [x] (js/console.log x) x) a 1 b 1] (count (identity-form #{(f a) (f b)}))) (catch js/Error e (js/console.log (.-message e))))";
    assert_eq!(
        execute(&compile_with_macros(source, &mut h).unwrap()),
        "1\n1\nDuplicate key: 1\n"
    );
    for source in [
        "(let [f (fn* [x] (js/console.log x) x)] (js/console.log (count #{(f 1) (f 1)})))",
        "(let* [f (fn* [x] (js/console.log x) x)] (js/console.log (count (identity-form #{(f 1) (f 1)}))))",
    ] {
        assert!(
            compile_with_macros(source, &mut h)
                .unwrap_err()
                .to_string()
                .contains("duplicate set member")
        );
    }
    for source in [
        "(identity-form #{1 1})",
        "(identity-form '#{x x})",
        "(ignore ^:m #{1 1})",
    ] {
        assert!(
            compile_with_macros(source, &mut h)
                .unwrap_err()
                .to_string()
                .contains("duplicate set member")
        );
    }
    let source = "; λ\n(identity-form #{:a missing})";
    let error = compile_with_macros(source, &mut h).unwrap_err();
    assert!(
        error.to_string().contains("unresolved symbol: missing"),
        "{error}"
    );
    let light_cljs::CompileError::Analysis(error) = error else {
        panic!()
    };
    assert_eq!(&source[error.span], "missing");
}

#[test]
fn macros_generate_resolved_reductions_and_fixed_callbacks_without_native_execution() {
    let mut h = host(
        "(ns macros) (defmacro sum [xs] (list 'cljs.core/reduce 'cljs.core/+ 0 xs)) (defmacro early [xs] (list 'cljs.core/reduce '(fn* [a b] (cljs.core/reduced (cljs.core/+ a b))) 0 xs))",
        &["sum", "early"],
    );
    let source = "(let [reduce 99 + 88] (js/console.log (sum [1 2 3]) (early [4 5 6]) (cljs.core/reduce (fn [a b] (cljs.core/+ a b)) 0 [7 8])))";
    assert_eq!(
        execute(&compile_with_macros(source, &mut h).unwrap()),
        "6 4 15\n"
    );
    assert_eq!(
        result(&compile("(reduce (fn [a b] (+ a b)) 0 [1 2 3])").unwrap()),
        "6\n"
    );
    let acceptance = "(let [observe (fn [x] (js/console.log x))] (= [(reduce + [1 2 3]) (reduce + 10 [1 2 3]) (reduce + []) (reduce + 7 []) (reduce (fn [] 17) []) (reduce (fn [a b] (observe 99)) [5]) (reduce conj [] (list 1 2))] [6 16 0 7 17 5 [1 2]]))";
    assert_eq!(result(&compile(acceptance).unwrap()), "true\n");
}

#[test]
fn unsupported_generated_collection_program_forms_keep_file_macro_and_callsite_diagnostics() {
    let sources = Sources::new();
    for body in [
        "'(lazy-seq missing)",
        "'(defrecord Thing [x x])",
        "'(defprotocol P (method []))",
    ] {
        sources.write(
            "bad/macros.cljs",
            &format!("(ns bad.macros) (defmacro emit-bad [] {body})"),
        );
        let source = "(ns bad.main (:require [bad.macros :as m]))\n; λ\n(m/emit-bad)";
        sources.write("bad/main.cljs", source);
        let error = sources.compile("bad.main").unwrap_err();
        assert!(error.message.contains("m/emit-bad"), "{body}: {error}");
        assert!(error.path.as_ref().unwrap().ends_with("bad/main.cljs"));
        assert_eq!(error.line_column, Some((3, 1)), "{body}: {error}");
        assert_eq!(
            error.span,
            Some(source.find("(m/emit-bad)").unwrap()..source.len())
        );
        let output = Command::new(env!("CARGO_BIN_EXE_light-cljs"))
            .args(["--compile", "--entry", "bad.main", "--source-root"])
            .arg(&sources.0)
            .output()
            .unwrap();
        assert!(!output.status.success(), "{body}");
        assert!(output.stdout.is_empty(), "{body}: partial JS");
        let diagnostic = String::from_utf8(output.stderr).unwrap();
        assert!(
            diagnostic.contains("bad/main.cljs:3:1:"),
            "{body}: {diagnostic}"
        );
        assert!(diagnostic.contains("m/emit-bad"), "{body}: {diagnostic}");
    }
}

#[test]
fn generated_qualified_operations_keep_core_identity_under_lexical_and_global_shadowing() {
    let sources = Sources::new();
    sources.write(
        "batch/macros.cljs",
        include_str!("../examples/collections/batch/macros.cljs"),
    );
    sources.write(
        "user/data.cljs",
        "(ns user.data) (def assoc (fn* [x] x)) (def vector (fn* [x] x))",
    );
    sources.write(
        "main.cljs",
        r#"(ns main (:require [cljs.core :as c] [user.data :as u] [batch.macros :as m]))
      (def assoc (fn* [x] x))
      (def saved c/get)
      (def get (fn* [x] x))
      (let [conj (fn* [x] x) reduce (fn* [x] x) + (fn* [x] x)
            changed (m/append-item {:items [1 2]} :items 3)]
        (js/console.log (c/reduce c/+ 0 (saved changed :items))
          (assoc 7) (get 8) (u/assoc 9) (u/vector 10) (conj 11) (reduce 12) (+ 13)
          (namespace ::m/x)))"#,
    );
    assert_eq!(
        execute(&sources.compile("main").unwrap()),
        "6 7 8 9 10 11 12 13 batch.macros\n"
    );
}

#[test]
fn collection_macro_compilation_obeys_shared_limits_and_recovers_with_the_same_host() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/collections");
    let mut host = MacroHost::new();
    host.evaluator_options.poll_budget = 0;
    let error = compile_namespace_with_macros("batch.main", std::slice::from_ref(&root), &mut host)
        .unwrap_err();
    assert!(error.to_string().contains("budget"), "{error}");
    host.evaluator_options = Default::default();
    let js = compile_namespace_with_macros("batch.main", &[root], &mut host).unwrap();
    assert_eq!(
        execute(&format!("const run = {js}; console.log(typeof run);")),
        "data\nops\nmain\nfunction\n"
    );
}

#[test]
fn shared_macros_and_runtime_refers_preserve_renamed_identity() {
    let s = Sources::new();
    s.write(
        "shared.cljc",
        r#"(ns shared) (def value 8)
       #?(:cljs (defmacro add [x] `(cljs.core/+ ~x 3)) :clj jvm-only)"#,
    );
    s.write(
        "main.cljs",
        r#"(ns main (:require [shared :as s :refer [value add] :rename {add plus}]))
        (js/console.log value (plus 4) (s/add 6) (= ::s/k :shared/k))"#,
    );
    assert_eq!(execute(&s.compile("main").unwrap()), "8 7 9 true\n");
}

#[test]
fn legacy_macro_import_syntax_has_located_migration_diagnostics() {
    let s = Sources::new();
    for clause in [
        "(:require-macros [lib :as l])",
        "(:use-macros [lib :only [add]])",
        "(:require [lib :refer-macros [add]])",
        "(:require [lib :include-macros true])",
        "(:require [lib :include-macros false])",
    ] {
        s.write("main.cljs", &format!("(ns main\n  {clause}) 1"));
        let error = s.compile("main").unwrap_err();
        assert!(error.message.contains("retired"), "{error}");
        assert!(error.message.contains("ordinary :require"), "{error}");
        assert_eq!(error.line_column.unwrap().0, 2);
    }
}

#[test]
fn companion_files_alone_do_not_enable_inference_and_generated_errors_stay_located() {
    let s = Sources::new();
    s.write("lib.cljs", "(ns lib)");
    s.write("lib.clj", "(ns lib) (defmacro bad [] '(try (recur)))");
    s.write("main.cljs", "(ns main (:require [lib :as l])) (l/bad)");
    assert!(
        s.compile("main")
            .unwrap_err()
            .message
            .contains("unresolved symbol")
    );
    s.write("lib.cljs", "(ns lib) (defmacro bad [] '(try (recur)))");
    s.write("main.cljs", "(ns main (:require [lib :as l]))\n(l/bad)");
    let error = s.compile("main").unwrap_err();
    assert_eq!(error.line_column, Some((2, 1)));
    assert!(error.message.contains("recur"));
}

#[test]
fn shared_conditional_arguments_are_selected_before_macro_lowering() {
    let s = Sources::new();
    s.write("lib.cljs", "(ns lib) (defmacro id [x] x)");
    s.write(
        "main.cljc",
        "(ns main (:require [lib :refer [id]])) (js/console.log (id #?(:clj jvm-symbol :cljs 7)))",
    );
    assert_eq!(execute(&s.compile("main").unwrap()), "7\n");
}

#[test]
fn namespace_metadata_and_docstrings_reach_the_macro_environment_and_refresh_on_reuse() {
    let s = Sources::new();
    s.write("macros.cljs", "(ns macros) (defmacro read-meta [] (str (:foo (meta (:name (:ns &env)))) \"/\" (:doc (:ns &env))))");
    s.write("main.cljs", "(ns ^{:foo old} main \"doc\" {:foo new} (:require [macros :refer [read-meta]])) (js/console.log (read-meta))");
    let mut h = MacroHost::new();
    assert_eq!(
        execute(&s.compile_host("main", &mut h).unwrap()),
        "new/doc\n"
    );
    s.write(
        "main.cljs",
        "(ns main (:require [macros :refer [read-meta]])) (js/console.log (read-meta))",
    );
    assert_eq!(execute(&s.compile_host("main", &mut h).unwrap()), "/\n");
}

#[test]
fn explicit_use_and_core_require_preserve_renamed_namespace_identity() {
    let s = Sources::new();
    s.write("lib.cljs", "(ns lib) (def f (fn* [] 123))");
    s.write("main.cljs", "(ns main (:use [lib :only [f] :rename {f renamed}]) (:require [clojure.core :refer [when] :rename {when always}])) (js/console.log (renamed) (= renamed lib/f) (always true 42))");
    assert_eq!(execute(&s.compile("main").unwrap()), "123 true 42\n");
    for spec in ["[lib]", "[lib :refer [f]]", "[lib :only [f] :as l]"] {
        s.write("main.cljs", &format!("(ns main (:use {spec})) 0"));
        assert!(s.compile("main").is_err());
    }
}

#[test]
fn overload_macro_environments_keep_method_parameters_and_rest_argument_identity() {
    let mut h = host(
        "(ns macros) (defmacro local-kind [name] (get-in &env [:locals name :local]))",
        &["local-kind"],
    );
    let source = "(let [f (fn f ([] (local-kind f)) ([x & xs] [(local-kind x) (local-kind xs)]))] (= [:fn [:arg :arg]] [(f) (f 1 2)]))";
    assert_eq!(
        result(&compile_with_macros(source, &mut h).unwrap()),
        "true\n"
    );
}

#[test]
fn imported_macro_environment_keeps_qualified_ids_across_a_long_shadow_chain() {
    let s = Sources::new();
    s.write(
        "env/tools.cljs",
        r#"(ns env.tools)
(def hits (atom 0))
(defmacro capture []
  (swap! hits inc)
  (list 'quote
    [(get-in &env [:locals 'x :light-cljs/id])
     (get-in &env [:locals 'x :shadow :light-cljs/id])
     (get-in &env [:locals 'x :shadow :shadow :light-cljs/id])
     (get-in &env [:locals 'x :shadow :shadow :shadow :light-cljs/id])
     (get-in &env [:locals 'x :shadow :shadow :shadow :shadow])
     (get-in &env [:locals 'x :shadow :shadow :arg-id])
     (:context &env) (get-in &env [:ns :name])
     (get-in &env [:ns :requires 'e])
     (:line &env) (:column &env) &form @hits]))"#,
    );
    s.write(
        "app.cljs",
        "(ns app (:require [env.tools :as e]))
(pr-str
 [(let* [x 10]
    ((fn* [x]
       (let* [x 30 x 40]
         (e/capture))) 20))
  @e/hits])",
    );
    let expected = "[[3 2 1 0 nil 0 :return app env.tools 6 10 (e/capture) 1] 0]";
    let mut h = MacroHost::new();
    for _ in 0..2 {
        let js = s.compile_host("app", &mut h).unwrap();
        assert_eq!(result(&js), format!("{expected}\n"));
        // Application evaluation owns a fresh heap: macro mutation is quoted
        // into the expansion, while the imported application atom stays zero.
        let e = light_cljs::evaluator::Evaluator::new(Default::default()).unwrap();
        let value = e.evaluate_js(&js).unwrap();
        assert_eq!(e.string(value).unwrap(), expected);
    }
}

#[test]
fn macros_emit_mutual_letfn_with_the_complete_lexical_environment() {
    let mut h = host(
        r#"(ns macros)
      (defmacro group [x]
        `(letfn [(f# ([] (g# 0)) ([n#] (g# n#)))
                 (g# [n#] (+ ~x n#))]
           [(f#) (f# 2)]))
      (defmacro local-kind [name] (get-in &env [:locals name :local]))"#,
        &["group", "local-kind"],
    );
    assert_eq!(
        result(&compile_with_macros("(let [x 10] (= (group x) [10 12]))", &mut h).unwrap()),
        "true\n"
    );
    assert_eq!(
        result(
            &compile_with_macros(
                "(letfn [(f [] (local-kind g)) (g [] 2)] (= :letfn (f)))",
                &mut h
            )
            .unwrap()
        ),
        "true\n"
    );
}

#[test]
fn generated_destructured_methods_and_qualified_assertions_execute_in_javascript() {
    let mut h = host(
        "(ns macros) (defmacro bundle [] '(do (defprotocol P (run [this input])) (defrecord R [a] P (run [_ {:keys [b]}] (+ a b))) (run (->R 2) {:b 3}))) (defmacro checked [] '(fn [[x]] {:pre [(pos? x)] :post [(pos? %)]} x))",
        &["bundle", "checked"],
    );
    assert_eq!(
        result(&compile_with_macros("(bundle)", &mut h).unwrap()),
        "5\n"
    );
    assert_eq!(
        result(&compile_with_macros("((checked) [7])", &mut h).unwrap()),
        "7\n"
    );
    let sources = Sources::new();
    sources.write("qualified.cljs", "(ns qualified (:require [cljs.core :as c])) (c/defprotocol P (run [this input])) (c/defrecord R [x] P (run [_ {:keys [a]}] (c/assert (pos? a)) (+ x a))) (js/console.log (run (->R 2) {:a 3}))");
    assert_eq!(execute(&sources.compile("qualified").unwrap()), "5\n");
}

#[test]
fn generated_exception_forms_keep_default_catches_and_original_values() {
    let mut h = host(
        "(ns macros) (defmacro checked [] '(fn [effect] (try (throw (cljs.core/ex-info \"bad\" {:a 1})) (catch :default e (effect 1) (cljs.core/ex-message e)) (finally (effect 2)))))",
        &["checked"],
    );
    let code = compile_with_macros(
        "(let [trace (array) f (checked)] (array (f (fn [x] (.push trace x))) trace))",
        &mut h,
    )
    .unwrap();
    assert_eq!(
        execute(&format!("console.log(JSON.stringify({code}));")),
        "[\"bad\",[1,2]]\n"
    );
    let sources = Sources::new();
    sources.write("errors.cljs", "(ns errors (:require [cljs.core :as c])) (let [e (c/ex-info \"bad\" {:a 1})] (try (throw e) (catch c/ExceptionInfo caught (js/console.log (c/identical? e caught) (c/ex-message caught)))))");
    assert_eq!(execute(&sources.compile("errors").unwrap()), "true bad\n");
}

#[test]
fn control_macros_execute_directly_and_from_source_macro_namespaces() {
    let sources = Sources::new();
    sources.write("controls.cljs", "(ns controls) (def n (atom 0)) (defmacro marked [] (swap! n inc)) (defmacro generated [] '(let [out (array)] (while (< (.-length out) 3) (.push out (.-length out))) (dotimes [i 2.5] (.push out i)) out)) (defmacro compiled-count [] (let [a (atom 0)] (dotimes [i 2.5] (swap! a inc)) @a)) (defmacro enabled [] true)");
    sources.write("main.cljs", "(ns main (:require [cljs.core :as c] [controls :as m] [cljs.core :refer [assert] :rename {assert check}])) (js/console.log (.-length (m/generated)) (m/compiled-count) (m/enabled) (str (c/condp = 2 2 :ok :bad)) (case 1 (1 2 3) (m/marked) 0) (m/marked) (check true))");
    assert_eq!(
        execute(&sources.compile("main").unwrap()),
        "6 3 true :ok 1 2 null\n"
    );
}

#[test]
fn compiler_assertion_configuration_removes_conditions_messages_and_prepost_checks() {
    let mut host = MacroHost::new();
    assert!(host.assertions_enabled());
    host.set_assertions(false);
    assert!(!host.assertions_enabled());
    let source = "(js/console.log (assert missing-condition missing-message) ((fn [x] {:pre [missing-pre] :post [missing-post]} x) 7))";
    let js = compile_with_macros(source, &mut host).unwrap();
    assert_eq!(execute(&js), "null 7\n");
    assert!(!js.contains("Assert failed"));
    assert!(!js.contains("const rt ="));
    let sources = Sources::new();
    sources.write("config.cljs", "(ns config) (defmacro enabled [] '(do (assert false) false)) (defmacro guarded [] '(assert false)) (defmacro compiled-check [] (assert false) 3) (defmacro expanded [] '(assert false))");
    sources.write(
        "main.cljs",
        "(ns main (:require [config :as c])) (js/console.log (c/enabled) (c/guarded) (c/compiled-check) (c/expanded))",
    );
    assert_eq!(
        execute(&sources.compile_host("main", &mut host).unwrap()),
        "false null 3 null\n"
    );
    host.set_assertions(true);
    assert!(compile_with_macros(source, &mut host).is_err());
    assert!(MacroHost::new().assertions_enabled());
}

#[test]
fn control_iteration_captures_have_direct_expected_values_and_do_not_leak_recur() {
    // The historical Number-based duplicate rejection is retained in the
    // pre-repair receipt; independent pins require first-match acceptance.
    assert_eq!(
        result(&compile("(str (case 1 1 :integer 1.0 :float))").unwrap()),
        ":integer\n"
    );
    let primitive_case = compile("(case 1 1 2 3)").unwrap();
    assert!(!primitive_case.contains("const rt ="));
    assert!(primitive_case.contains("switch (caseValue)"));
    assert_eq!(result(&primitive_case), "2\n");
    let js = compile("(let [out #js []] (dotimes [i 4] (.push out (fn [] i))) out)").unwrap();
    assert_eq!(
        execute(&format!(
            "const fs = {js}; console.log(JSON.stringify(fs.map(f => f())));"
        )),
        "[0,1,2,3]\n"
    );
    let while_js = compile("(let [out #js []] (while (< (.-length out) 4) (let [i (.-length out)] (.push out (fn [] i)))) out)").unwrap();
    assert_eq!(
        execute(&format!(
            "const fs = {while_js}; console.log(JSON.stringify(fs.map(f => f())));"
        )),
        "[0,1,2,3]\n"
    );
    for source in [
        "(while true (recur))",
        "(dotimes [i 3] (recur 2))",
        "(fn [x] (while x (recur false)))",
        "(loop [x 0] (dotimes [i 3] (+ 1 (recur 0))))",
        "(case 1 1 :a 1 :b)",
        "(case 1 (1 2) :a 2 :b)",
        "(case [1] [1] :a [1] :b)",
        "(case* 1 [[1]] [2] 3)",
        "(let [x 1] (case* x [1] [2] 3))",
        "(let [x 1] (case* x [[:a]] [2] 3))",
        "(let [x 1 y 1] (case* x [[y]] [2] 3))",
        "(if-some [x 1 y 2] x)",
        "(when-let (x 1) x)",
        "(cond true)",
        "(cond-> 1 true)",
    ] {
        assert!(compile(source).is_err(), "accepted {source}");
    }
    assert_eq!(
        result(&compile("(def ^:const one 1) (let [x 1] (case* x [[one]] [42] 0))").unwrap()),
        "42\n"
    );
}

#[test]
fn generated_metadata_evaluates_runtime_forms_and_preserves_macro_definition_metadata() {
    let mut macros = host(
        r#"(ns macros)
        (defmacro decorated [x]
          (with-meta [x] {:value (list '+ x 1) :line 99 :file "macro.cljs"}))
        (defmacro quoted [] (list 'quote (with-meta 'sample {:marker true :line 99})))
        (defmacro ^{:purpose "compiler"} definition [] (:purpose (get (:light-cljs/macros &env) 'macros/definition)))"#,
        &["decorated", "quoted", "definition"],
    );
    let source = r#"(let [x 4 v (decorated x) s (quoted)]
        (and (= [4] v) (= {:value 5} (meta v))
             (= 'sample s) (= {:marker true} (meta s))
             (= "compiler" (definition))))"#;
    assert_eq!(
        result(&compile_with_macros(source, &mut macros).unwrap()),
        "true\n"
    );
}

// Compiler/QuickJS bridge observations, with emitted macro functions and source
// declarations. The numeric fixtures retain their pinned expected outcomes.
mod javascript_form_bridge {
    use super::*;
    use light_cljs::{
        analyzer::{ExprContext, analyze_compiler_form},
        compiler_data::{
            Datum, DatumKind, ExpansionOrigin, Form, Name as CompilerName, Origin, Source,
        },
        evaluator::{Evaluator, EvaluatorOptions},
        form_bridge::ConversionLimits,
    };
    fn read(text: &str) -> Form {
        let source = Source::new("caller.cljs", text);
        MacroHost::new()
            .read_compiler_form(&source)
            .unwrap()
            .unwrap()
    }
    fn origin(call: &Form) -> ExpansionOrigin {
        let definition = Source::new("macros.cljs", "(defmacro identity [x] x)");
        ExpansionOrigin {
            macro_name: CompilerName::new(Some("macros"), "identity"),
            call: call.span(),
            definition: definition.span(0..definition.text().len()),
            helpers: vec![CompilerName::new(Some("macros"), "helper")],
        }
    }
    fn function(e: &Evaluator, text: &str) -> light_cljs::evaluator::ValueId {
        e.evaluate(&light_cljs::analyze(text, &parse(text).unwrap()).unwrap())
            .unwrap()
    }
    fn execute_form(e: &Evaluator, form: &Form) -> light_cljs::evaluator::ValueId {
        e.evaluate(&analyze_compiler_form(form).unwrap()).unwrap()
    }
    #[test]
    fn pinned_numeric_bodies_execute_inside_the_javascript_macro_bridge() {
        let e = Evaluator::new(EvaluatorOptions::default()).unwrap();
        e.evaluate_js(&format!(
            "({})(rt)",
            include_str!("support/numeric_observation.js")
        ))
        .unwrap();
        let call = read("(macros/identity)");
        let mut host = MacroHost::new();
        let mut count = 0;
        for row in include_str!("fixtures/execution-outcomes.tsv").lines() {
            if !row.starts_with("numeric-") {
                continue;
            }
            let columns: Vec<_> = row.split('\t').collect();
            let [name, body, outcome, trace, stdout] = columns.as_slice() else {
                panic!("invalid numeric fixture");
            };
            assert_eq!(*stdout, "\"\"");
            let namespace = format!("semantic.case{count}");
            let source = format!(
                "(ns {namespace} (:require [cljs.core :as c]))\n\
                 (fn* [form env] (js/numericObservation (fn* [observe] {body})))"
            );
            let syntax = parse(&source).unwrap();
            let p = light_cljs::analyzer::analyze_namespaces_with_macros(
                &[light_cljs::analyzer::NamespaceInput {
                    name: &namespace,
                    source: &source,
                    syntax: &syntax,
                    aliases: &[("c".into(), "cljs.core".into())],
                    requires: &["cljs.core".into()],
                }],
                &mut host,
            )
            .unwrap_or_else(|(_, error)| panic!("{name}: {body}: {error}"));
            let f = e.evaluate(&p).unwrap();
            let expansion = host
                .expand_javascript(&e, f, &call, &Datum::new(DatumKind::Nil), origin(&call))
                .unwrap_or_else(|error| panic!("{name}: {error}"));
            let DatumKind::String(text) = expansion.data().kind().clone() else {
                panic!("{name}: numeric observation was not a string");
            };
            assert_eq!(
                String::from_utf16(&text).unwrap(),
                format!("{{\"outcome\":{outcome},\"trace\":{trace},\"stdout\":{stdout}}}"),
                "{name}: {body}"
            );
            e.release(f).unwrap();
            count += 1;
        }
        assert_eq!(count, 57);
    }

    #[test]
    fn number_expansion_data_matches_independent_application_heaps() {
        let cases = [
            (
                "[(/ 1 2) (= 1 1.0) (inc 9007199254740992) (- (+ 9007199254740992 1) 9007199254740992) (/ 1 -0.0) (/ 1 0) (js/isNaN (/ 0 0)) (infinite? (/ 1 0))]",
                "[0.5 true 9007199254740992 0 ##-Inf ##Inf true true]",
            ),
            (
                "[(int 4294967297.9) (long 4294967297.9) (byte 300) (short 65537) (float 16777217) (double 16777217) (quot 2 -5) (rem -0.0 2) (mod 4 -2) (== 1 1.0) (/ 1 (long -0.4))]",
                "[1 4294967297 300 65537 16777217 16777217 -0.0 -0.0 -0.0 true ##-Inf]",
            ),
            (
                "[(get {1 :ok} 1.0) (count (conj #{1} 1.0)) (get {[1 {:n 2}] :nested} [1.0 {:n 2.0}]) (= (hash [1 {:n 2}]) (hash [1.0 {:n 2.0}])) (get {-0.0 :zero} 0) (= (hash -0.0) (hash 0)) (get {##Inf :positive ##-Inf :negative} (/ 1 0)) (= (hash ##Inf) (hash (/ 1 0))) (= ##NaN ##NaN) (get {##NaN :nan} ##NaN :missing) (count (hash-set ##NaN ##NaN)) (get {[##NaN] :nan} [##NaN] :missing)]",
                "[:ok 1 :nested true :zero true :positive true false :missing 2 :missing]",
            ),
            (
                "(let [f + cmp < calls (atom []) observe (fn [x] (swap! calls conj x) x)] [(apply f [9007199254740992 1 1]) ((nth [inc] 0) 9007199254740992) (mapv inc (range 3)) (number? (count [1])) (number? (nth (range 3) 1)) (number? (first (keep-indexed (fn [i _] i) [:x]))) (name (symbol (str (/ 1 2)))) (name (symbol (pr-str (float 16777217)))) (cmp (observe 9) (observe 4) (observe 2)) (< (observe 8) (observe 3) (observe 1)) @calls])",
                "[9007199254740992 9007199254740992 [1 2 3] true true true \"0.5\" \"16777217\" false false [9 4 2 8 3 1]]",
            ),
        ];
        for (body, expected) in cases {
            let source = format!("(defmacro capture [] (list 'quote {body})) [(capture) {body}]");
            let mut host = MacroHost::new();
            let syntax = parse(&source).unwrap();
            let p = analyze_with_macros(&source, &syntax, &mut host).unwrap();
            let expected = format!("[{expected} {expected}]");
            let app = Evaluator::new(EvaluatorOptions::default()).unwrap();
            assert_eq!(
                app.print(app.evaluate(&p).unwrap()).unwrap(),
                expected,
                "{body}"
            );
            let script = format!(
                "const rt = {}; console.log(rt.core.prStr(({})(rt)));",
                light_cljs::evaluator::RUNTIME_SOURCE,
                light_cljs::emitter::emit_js_unit(&p)
            );
            assert_eq!(execute(&script).trim(), expected, "{body}");
        }
    }

    #[test]
    fn numeric_macro_failures_keep_call_definition_and_helper_provenance() {
        let e = Evaluator::new(EvaluatorOptions::default()).unwrap();
        let f = function(&e, "(fn* [form env] (even? 1.5))");
        let call = read("\n(macros/identity)");
        let error = MacroHost::new()
            .expand_javascript(&e, f, &call, &Datum::new(DatumKind::Nil), origin(&call))
            .unwrap_err();
        for part in [
            "caller.cljs:2:1",
            "macros/identity",
            "macros.cljs",
            "macros/helper",
            "integer",
        ] {
            assert!(error.contains(part), "missing {part:?}: {error}");
        }
        let mut host = MacroHost::new();
        assert!(compile_with_macros("(defmacro bad [] (even? 1.5)) (bad)", &mut host).is_err());
        assert!(compile_with_macros("(defmacro good [] (/ 1 2)) (good)", &mut host).is_ok());
    }

    #[test]
    fn reader_numeric_spellings_are_number_values_before_macro_execution() {
        let source = r#"
            (defmacro literal [x] x)
            (defmacro inspect [x] [(number? x) (float? x) (double? x) (= x 1.0)])
            [(literal 9007199254740993) (literal 9007199254740993N)
             (literal 16777217M) (literal 4/2) (/ 1 (literal -0.0))
             (js/isNaN (literal ##NaN)) (infinite? (literal ##Inf))
             (infinite? (literal ##-Inf)) (inspect 1)]"#;
        let mut host = MacroHost::new();
        let p = analyze_with_macros(source, &parse(source).unwrap(), &mut host).unwrap();
        let e = Evaluator::new(EvaluatorOptions::default()).unwrap();
        assert_eq!(
            e.print(e.evaluate(&p).unwrap()).unwrap(),
            "[9007199254740992 9007199254740992 16777217 2 ##-Inf true true true [true true true true]]"
        );
        assert!(compile_with_macros("(defmacro literal [x] x) (literal 1/2)", &mut host).is_err());
    }
    #[test]
    fn identity_preserves_owned_forms_and_executes_returned_code() {
        let e = Evaluator::new(EvaluatorOptions::default()).unwrap();
        let id = function(&e, "(fn* [form env x] x)");
        let call = read(
            r#"(macros/identity '^:kept [foo :foo () [] {1 :one [2] :vector {3 4} :map} #{1 [2]} "\ud800" #"a" #inst "2000" #uuid "1-1-1-1-1" #js [1] #js {:x 2}])"#,
        );
        let mut host = MacroHost::new();
        let output = host
            .expand_javascript(&e, id, &call, &Datum::new(DatumKind::Nil), origin(&call))
            .unwrap();
        assert_eq!(output.span().bytes(), call.children()[1].span().bytes());
        assert_eq!(output.node().origin, call.children()[1].node().origin);
        assert!(output.children()[1].metadata().is_some());
        let value = execute_form(&e, &output);
        let text = e.print(value).unwrap();
        assert!(
            text.starts_with("[foo :foo () [] {1 :one, [2] :vector, {3 4} :map} #{1 [2]}"),
            "{text}"
        );
        let inspect = function(
            &e,
            "(fn* [v] [(symbol? (nth v 0)) (keyword? (nth v 1)) (list? (nth v 2)) (vector? (nth v 3)) (get (nth v 4) [2]) (contains? (nth v 5) [2]) (.charCodeAt (nth v 6) 0) (.-x (nth v 11))])",
        );
        assert_eq!(
            e.print(e.call(inspect, &[value]).unwrap()).unwrap(),
            "[true true true true :vector true 55296 2]"
        );
        e.collect_garbage();
        assert_eq!(e.print(value).unwrap(), text);
        drop(e);
        assert_eq!(
            output.expansions()[0]
                .definition
                .as_ref()
                .unwrap()
                .source()
                .name(),
            "macros.cljs"
        );
    }
    #[test]
    fn unchanged_scalar_arguments_keep_their_original_location() {
        for text in [
            "(macros/identity -0.0)",
            "(macros/identity \"\\ud800\")",
            "(macros/identity true)",
            "(macros/identity nil)",
        ] {
            let e = Evaluator::new(EvaluatorOptions::default()).unwrap();
            let call = read(text);
            let f = function(&e, "(fn* [form env x] x)");
            let output = MacroHost::new()
                .expand_javascript(&e, f, &call, &Datum::new(DatumKind::Nil), origin(&call))
                .unwrap();
            assert_eq!(output.span().bytes(), call.children()[1].span().bytes());
            assert_eq!(output.node().origin, call.children()[1].node().origin);
        }
    }
    #[test]
    fn generated_code_executes_in_order_and_preserves_number_bits() {
        let e = Evaluator::new(EvaluatorOptions::default()).unwrap();
        let call = read("\n(macros/identity)");
        let f = function(
            &e,
            r#"(fn* [form env] (list 'let* ['a (list 'atom [])] (list 'do (list 'swap! 'a 'conj 1) (list 'swap! 'a 'conj 2) (list 'vector (list 'deref 'a) ##NaN ##Inf ##-Inf -0.0))))"#,
        );
        let output = MacroHost::new()
            .expand_javascript(&e, f, &call, &Datum::new(DatumKind::Nil), origin(&call))
            .unwrap();
        assert!(matches!(output.node().origin, Origin::MacroExpansion(_)));
        assert_eq!(output.span().bytes(), call.span().bytes());
        let result = execute_form(&e, &output);
        let check = function(
            &e,
            "(fn* [v] [(nth v 0) (js/Number.isNaN (nth v 1)) (= (nth v 2) ##Inf) (= (nth v 3) ##-Inf) (js/Object.is (nth v 4) -0.0)])",
        );
        assert_eq!(
            e.print(e.call(check, &[result]).unwrap()).unwrap(),
            "[[1 2] true true true true]"
        );
    }
    #[test]
    fn form_and_bounded_environment_use_actual_nested_call_and_shadow_ids() {
        let e = Evaluator::new(EvaluatorOptions::default()).unwrap();
        let call = read("(outer (macros/identity))").children()[1].clone();
        let mut host = MacroHost::new();
        let env = host
            .compiler_environment(
                "app",
                ExprContext::Return,
                &[
                    ("x".into(), 1, "let", None),
                    ("x".into(), 2, "arg", Some(0)),
                ],
                &[("t".into(), "tools".into())],
                &["tools".into()],
                (1, 8),
            )
            .unwrap();
        let f = function(
            &e,
            "(fn* [form env] (list 'quote [form (get-in env [:locals 'x :light-cljs/id]) (get-in env [:locals 'x :shadow :light-cljs/id]) (get-in env [:locals 'x :arg-id]) (:context env) (get-in env [:ns :requires 't]) (:line env) (:column env)]))",
        );
        let output = host
            .expand_javascript(&e, f, &call, &env, origin(&call))
            .unwrap();
        assert_eq!(
            e.print(execute_form(&e, &output)).unwrap(),
            "[(macros/identity) 2 1 0 :return tools 1 8]"
        );
        assert_eq!(
            output.children()[1].children()[0].span().bytes(),
            call.span().bytes()
        );
    }
    #[test]
    fn qualified_environment_keys_keep_a_long_shadow_chain_and_binding_shape() {
        let e = Evaluator::new(EvaluatorOptions::default()).unwrap();
        let call = read("(outer (macros/identity))").children()[1].clone();
        let mut host = MacroHost::new();
        let env = host
            .compiler_environment(
                "app",
                ExprContext::Return,
                &[
                    ("x".into(), 5, "fn", None),
                    ("x".into(), 6, "arg", Some(0)),
                    ("x".into(), 7, "let", None),
                    ("x".into(), 8, "let", None),
                ],
                &[],
                &[],
                (1, 8),
            )
            .unwrap();
        let f = function(
            &e,
            r#"(fn* [form env]
              (let* [a (get-in env [:locals 'x])
                     b (:shadow a) c (:shadow b) d (:shadow c)]
                (list 'quote
                  [form (mapv :light-cljs/id [a b c d]) (:shadow d)
                   (mapv :local [a b c d]) (mapv :binding-form? [a b c d])
                   (:arg-id c) (mapv :op [a b c d])
                   (mapv :name [a b c d]) (mapv :form [a b c d])
                   (mapv (fn [k] [(namespace k) (name k)])
                     (filter (fn [k] (= "id" (name k))) (keys a)))])))"#,
        );
        let output = host
            .expand_javascript(&e, f, &call, &env, origin(&call))
            .unwrap();
        let app = Evaluator::new(EvaluatorOptions::default()).unwrap();
        assert_eq!(
            app.print(execute_form(&app, &output)).unwrap(),
            "[(macros/identity) [8 7 6 5] nil [:let :let :arg :fn] [true true true nil] 0 [:binding :binding :binding :binding] [x x x x] [x x x x] [[\"light-cljs\" \"id\"]]]"
        );
        assert_eq!(
            output.children()[1].children()[0].span().bytes(),
            call.span().bytes()
        );
    }
    #[test]
    fn nested_services_share_forms_and_survive_gc_without_rust_collection_callbacks() {
        use std::{cell::RefCell, rc::Rc};
        let e = Evaluator::new(EvaluatorOptions::default()).unwrap();
        let call = read("(macros/identity [40 2])");
        let seen = Rc::new(RefCell::new(vec![]));
        let record = seen.clone();
        let span = call.span();
        e.install_form_services(
            call.clone(),
            origin(&call),
            ConversionLimits::default(),
            move |control, form, all| {
                control.poll()?;
                record.borrow_mut().push(all);
                if all {
                    Ok(Form::from_data(
                        Datum::new(DatumKind::Double(42.0)),
                        span.clone(),
                        Origin::SuppliedData,
                    ))
                } else {
                    Ok(form)
                }
            },
        )
        .unwrap();
        e.install_gc_callback("bridgeGC").unwrap();
        let f = function(
            &e,
            "(fn* [form env x] (let* [v (macroexpand-1 x)] (js/bridgeGC) (macroexpand (conj v 9))))",
        );
        let output = MacroHost::new()
            .expand_javascript(&e, f, &call, &Datum::new(DatumKind::Nil), origin(&call))
            .unwrap();
        assert_eq!(e.number(execute_form(&e, &output)).unwrap(), 42.0);
        assert_eq!(*seen.borrow(), vec![false, true]);
        e.collect_garbage();
        assert_eq!(e.number(execute_form(&e, &output)).unwrap(), 42.0);
    }
    #[test]
    fn unsupported_and_unbounded_results_are_located_and_fresh_sessions_recover() {
        for (body, category) in [
            ("(fn* [] 1)", "function"),
            ("(atom 0)", "atom"),
            ("(js/Promise.resolve 1)", "object"),
            ("(js/Symbol 'x)", "symbol"),
            ("(js/BigInt 1)", "bigint"),
            ("#js [1]", "object"),
            ("(range)", "limit"),
            ("(lazy-seq (throw (js/Error. \"bad\")))", "realization"),
            (
                "(js* \"(()=>{const x=rt.compilerForms.jsArray([]); x.push(x); return x;})()\")",
                "cyclic",
            ),
        ] {
            let e = Evaluator::new(EvaluatorOptions::default()).unwrap();
            // Runtime exposes the session-private explicit literal constructors
            // only through the evaluator embedding, not ordinary compiler globals.
            let f = if category == "cyclic" {
                e.evaluate_js(
                    "(()=>{const x=rt.compilerForms.jsArray([]);x.push(x);return ()=>x;})()",
                )
                .unwrap()
            } else {
                function(&e, &format!("(fn* [form env] {body})"))
            };
            let call = read("\n(macros/identity)");
            let mut host = MacroHost::new();
            host.limits.generated_nodes = 100;
            let error = host
                .expand_javascript(&e, f, &call, &Datum::new(DatumKind::Nil), origin(&call))
                .unwrap_err();
            assert!(error.contains(category), "{body}: {error}");
            for detail in [
                "caller.cljs:2:1",
                "macros/identity",
                "macros.cljs:1:1",
                "macros/helper",
                "$",
            ] {
                assert!(error.contains(detail), "{error}");
            }
            let fresh = host.new_evaluator().unwrap();
            assert_eq!(
                fresh.number(fresh.evaluate_js("42").unwrap()).unwrap(),
                42.0
            );
        }
    }
    #[test]
    fn definition_namespace_syntax_quote_and_gensym_are_hygienic() {
        let e = Evaluator::new(EvaluatorOptions::default()).unwrap();
        let source = Source::new("definition.cljs", "(fn* [form env x] `[::tag ~x])");
        let view = light_cljs::compiler_reader::NamespaceView::new("definition", &source);
        let form = light_cljs::compiler_reader::read_form(&source, &view)
            .unwrap()
            .unwrap();
        let f = e.evaluate(&analyze_compiler_form(&form).unwrap()).unwrap();
        let call = read("(macros/identity 7)");
        let env = MacroHost::new()
            .compiler_environment(
                "caller",
                ExprContext::Value,
                &[],
                &[("definition".into(), "wrong".into())],
                &[],
                (1, 1),
            )
            .unwrap();
        let output = MacroHost::new()
            .expand_javascript(&e, f, &call, &env, origin(&call))
            .unwrap();
        assert_eq!(
            e.print(execute_form(&e, &output)).unwrap(),
            "[:definition/tag 7]"
        );
        let f = function(
            &e,
            "(fn* [form env x] (let* [g (gensym)] (list 'let* [g 7] (list 'vector g (list 'quote x)))))",
        );
        let call = read("(macros/identity G__1)");
        let mut host = MacroHost::new();
        let a = host
            .expand_javascript(&e, f, &call, &Datum::new(DatumKind::Nil), origin(&call))
            .unwrap();
        let b = host
            .expand_javascript(&e, f, &call, &Datum::new(DatumKind::Nil), origin(&call))
            .unwrap();
        let symbol = |f: &Form| match f.children()[1].children()[0].data().kind() {
            DatumKind::Symbol(n) => n.clone(),
            _ => panic!(),
        };
        assert_ne!(symbol(&a), symbol(&b));
        assert_ne!(symbol(&a), CompilerName::new(None, "G__1"));
        assert_eq!(e.print(execute_form(&e, &a)).unwrap(), "[7 G__1]");
    }
    #[test]
    fn literal_mutations_and_constructed_metadata_decode_without_stale_origins() {
        let e = Evaluator::new(EvaluatorOptions::default()).unwrap();
        let call = read("(macros/identity #js {:x 1})");
        let f = function(&e, "(fn* [form env x] (do (aset x \"x\" 2) x))");
        let output = MacroHost::new()
            .expand_javascript(&e, f, &call, &Datum::new(DatumKind::Nil), origin(&call))
            .unwrap();
        assert!(matches!(output.node().origin, Origin::MacroExpansion(_)));
        let result = execute_form(&e, &output);
        let get = function(&e, "(fn* [x] (.-x x))");
        assert_eq!(e.number(e.call(get, &[result]).unwrap()).unwrap(), 2.0);
        let f = function(
            &e,
            "(fn* [form env x] (list 'quote (with-meta ['s :s] {:kept x})))",
        );
        let call = read("(macros/identity 9)");
        let output = MacroHost::new()
            .expand_javascript(&e, f, &call, &Datum::new(DatumKind::Nil), origin(&call))
            .unwrap();
        let result = execute_form(&e, &output);
        let get = function(&e, "(fn* [x] (:kept (meta x)))");
        assert_eq!(e.number(e.call(get, &[result]).unwrap()).unwrap(), 9.0);
        let f = e.evaluate_js("()=>rt.core.vector(new RegExp('a','gi'), new Date(0), rt.compilerForms.jsArray([1,2]))").unwrap();
        let output = MacroHost::new()
            .expand_javascript(&e, f, &call, &Datum::new(DatumKind::Nil), origin(&call))
            .unwrap();
        let result = execute_form(&e, &output);
        let get = function(
            &e,
            "(fn* [x] [(.-flags (nth x 0)) (inst-ms (nth x 1)) (aget (nth x 2) 1)])",
        );
        assert_eq!(
            e.print(e.call(get, &[result]).unwrap()).unwrap(),
            "[\"gi\" 0 2]"
        );
    }
    #[test]
    fn quoted_js_literals_preserve_nested_metadata_and_expansion_origins() {
        let e = Evaluator::new(EvaluatorOptions::default()).unwrap();
        let call = read("\n(macros/identity ^:kept [caller-symbol])");
        let f = function(
            &e,
            r#"(fn* [form env x]
              (let* [out '#js [#js {:x nil}]]
                (aset (aget out 0) "x" x)
                (list 'quote out)))"#,
        );
        let output = MacroHost::new()
            .expand_javascript(&e, f, &call, &Datum::new(DatumKind::Nil), origin(&call))
            .unwrap();
        let literal = output.children()[1].clone();
        assert!(matches!(
            literal.data().kind(),
            DatumKind::CompilerLiteral {
                kind: light_cljs::compiler_data::CompilerLiteralKind::JsArray,
                ..
            }
        ));
        assert_eq!(literal.span().bytes(), call.span().bytes());
        assert_eq!(literal.span().source().name(), call.span().source().name());
        assert!(matches!(literal.node().origin, Origin::MacroExpansion(_)));
        let borrowed = (0..output.nodes().len())
            .filter_map(|i| output.at(light_cljs::compiler_data::FormId(i)))
            .find(|form| {
                matches!(
                    form.data().kind(),
                    DatumKind::Collection {
                        kind: light_cljs::CollectionKind::Vector,
                        ..
                    }
                ) && form.span().bytes() == call.children()[1].span().bytes()
            })
            .unwrap();
        assert_eq!(borrowed.span().bytes(), call.children()[1].span().bytes());
        assert_eq!(borrowed.node().origin, call.children()[1].node().origin);
        assert!(borrowed.metadata().is_some());
        assert_eq!(output.expansions()[0].macro_name, origin(&call).macro_name);
        assert_eq!(
            output.expansions()[0]
                .definition
                .as_ref()
                .unwrap()
                .source()
                .name(),
            "macros.cljs"
        );
        assert_eq!(output.expansions()[0].helpers, origin(&call).helpers);
        let value = execute_form(&e, &output);
        let get = function(
            &e,
            r#"(fn* [x] (let* [v (.-x (aget x 0))]
              [(:kept (meta v)) (name (first v))]))"#,
        );
        assert_eq!(
            e.print(e.call(get, &[value]).unwrap()).unwrap(),
            "[true \"caller-symbol\"]"
        );

        // Embeddings may supply literal metadata directly through owned forms.
        let quoted = read("'#js [1]");
        let metadata = read("{:kept true}").data();
        let data = Datum::new(DatumKind::Collection {
            kind: light_cljs::CollectionKind::List,
            children: vec![
                quoted.children()[0].data(),
                quoted.children()[1].data().with_metadata(metadata),
            ]
            .into(),
        });
        let supplied = Form::from_data(data, quoted.span(), Origin::SuppliedData);
        let value = execute_form(&e, &supplied);
        let output = e.export_data(value, ConversionLimits::default()).unwrap();
        assert!(output.metadata().is_some());
        let get = function(&e, "(fn* [x] [(:kept (meta x)) (aget x 0)])");
        assert_eq!(e.print(e.call(get, &[value]).unwrap()).unwrap(), "[true 1]");
    }
    #[test]
    fn quoted_js_literal_export_is_bounded_and_unbranded_nested_values_are_rejected() {
        for (body, limits, detail) in [
            (
                "'#js [#js []]",
                ConversionLimits {
                    nodes: 3,
                    depth: 128,
                },
                "node limit",
            ),
            (
                "'#js [#js []]",
                ConversionLimits {
                    nodes: 100,
                    depth: 2,
                },
                "depth limit",
            ),
            (
                r#"(let* [out '#js [nil]] (aset out 0 (array 1)) out)"#,
                ConversionLimits::default(),
                "arbitrary object (including promise) at $[0]",
            ),
            (
                r#"(let* [out '#js {:x nil}] (aset out "x" (js-obj "y" 2)) out)"#,
                ConversionLimits::default(),
                "arbitrary object (including promise) at $.x",
            ),
        ] {
            let e = Evaluator::new(EvaluatorOptions::default()).unwrap();
            let f = function(&e, &format!("(fn* [] {body})"));
            let value = e.call(f, &[]).unwrap();
            let error = e.export_data(value, limits).unwrap_err().0;
            assert!(error.contains(detail), "{error}");
            let fresh = Evaluator::new(EvaluatorOptions::default()).unwrap();
            assert_eq!(
                fresh.number(fresh.evaluate_js("42").unwrap()).unwrap(),
                42.0
            );
        }
    }
    #[test]
    fn finite_lazy_results_realize_under_shared_control_and_handles_stay_session_owned() {
        let e = Evaluator::new(EvaluatorOptions::default()).unwrap();
        e.install_gc_callback("bridgeGC").unwrap();
        let call = read("(macros/identity)");
        let f = function(
            &e,
            "(fn* [form env] (cons 'vector (map (fn* [x] (js/bridgeGC) (inc x)) (range 40))))",
        );
        let output = MacroHost::new()
            .expand_javascript(&e, f, &call, &Datum::new(DatumKind::Nil), origin(&call))
            .unwrap();
        let result = execute_form(&e, &output);
        let count = function(&e, "(fn* [x] [(count x) (first x) (last x)])");
        assert_eq!(
            e.print(e.call(count, &[result]).unwrap()).unwrap(),
            "[40 1 40]"
        );
        let other = Evaluator::new(EvaluatorOptions::default()).unwrap();
        assert!(
            other
                .export_data(result, ConversionLimits::default())
                .unwrap_err()
                .0
                .contains("another")
        );
        assert_eq!(
            other.number(other.evaluate_js("42").unwrap()).unwrap(),
            42.0
        );
    }
    #[test]
    fn caught_nested_conversion_limits_cannot_reset_the_session_budget() {
        let e = Evaluator::new(EvaluatorOptions::default()).unwrap();
        let call = read("(macros/identity)");
        e.install_form_services(
            call.clone(),
            origin(&call),
            ConversionLimits {
                nodes: 100,
                depth: 128,
            },
            |_, form, _| Ok(form),
        )
        .unwrap();
        let f = function(
            &e,
            "(fn* [form env] (try (macroexpand (range)) (catch :default e 42)))",
        );
        let error = MacroHost::new()
            .expand_javascript(&e, f, &call, &Datum::new(DatumKind::Nil), origin(&call))
            .unwrap_err();
        assert!(error.contains("limit"), "{error}");
        assert!(e.evaluate_js("42").is_err());
    }
    #[test]
    fn rejection_never_calls_arbitrary_getters_or_to_string() {
        use std::sync::Arc;
        let hits = Arc::new(AtomicUsize::new(0));
        for expression in [
            "()=>rt.core.vector({get constructor(){probe('bad');return Object;},get then(){probe('bad');return null;},toString(){probe('bad');return 'object';}})",
            "()=>{const x=rt.compilerForms.jsArray([1]);Object.defineProperty(x,'0',{get(){probe('bad');return 1;}});return x;}",
            "()=>rt.core.vector(new Proxy({}, {get(){probe('bad');return 1;},getPrototypeOf(){probe('bad');return Object.prototype;}}))",
            "()=>new rt.core.LazySeq(null,()=>({get [Symbol.iterator](){probe('bad');return ()=>[];}}),null)",
        ] {
            let e = Evaluator::new(EvaluatorOptions::default()).unwrap();
            let h = hits.clone();
            e.install_callback("probe", move |_, _| {
                h.fetch_add(1, Ordering::Relaxed);
                Ok(String::new())
            })
            .unwrap();
            let f = e.evaluate_js(expression).unwrap();
            let call = read("(macros/identity)");
            let error = MacroHost::new()
                .expand_javascript(&e, f, &call, &Datum::new(DatumKind::Nil), origin(&call))
                .unwrap_err();
            assert!(error.contains("Unsupported macro result"), "{error}");
        }
        assert_eq!(hits.load(Ordering::Relaxed), 0);
    }
}

mod same_file_javascript_macros {
    use super::*;

    fn javascript_host() -> MacroHost {
        MacroHost::new()
    }

    #[test]
    fn explicit_quoted_js_forms_do_not_authorize_arbitrary_host_macro_results() {
        let mut host = javascript_host();
        for body in ["#js []", "#js {:x 1}", "(array 1)", r#"(js-obj "x" 1)"#] {
            let source = format!("(defmacro m [] {body})\n(m)");
            let error = light_cljs::compile_with_macros(&source, &mut host)
                .unwrap_err()
                .to_string();
            for detail in [
                "arbitrary object (including promise) at $",
                "cljs.user:2:1",
                "definition cljs.user:1:1",
            ] {
                assert!(error.contains(detail), "{body}: {error}");
            }
        }
        let source = "(defmacro m [] '(let [out #js []] out)) (m)";
        // Direct regression for the original program, after failed fresh sessions.
        assert_eq!(
            result(&format!(
                "JSON.stringify({})",
                light_cljs::compile_with_macros(source, &mut host).unwrap()
            )),
            "[]\n"
        );
    }
    #[test]
    fn compile_and_analyze_emit_expand_hygienically_without_runtime_macro_roots() {
        let source = r#"
          (defmacro twice [x] `(let [v# ~x] (+ v# v#)))
          (def calls (atom 0))
          (let [v 100 answer (twice (do (swap! calls inc) 21))]
            (and (= 42 answer) (= 1 @calls) (= 100 v)))"#;
        let mut host = javascript_host();
        assert_eq!(
            result(&compile_with_macros(source, &mut host).unwrap()),
            "true\n"
        );
        let program = analyze_with_macros(source, &parse(source).unwrap(), &mut host).unwrap();
        assert!(
            program
                .globals
                .iter()
                .all(|binding| binding.name != "twice")
        );
        assert_eq!(result(&emit_js(&program)), "true\n");
        assert_eq!(
            result(&compile("(defmacro twice [x] `(+ ~x ~x)) (twice 21)").unwrap()),
            "42\n"
        );
        assert_eq!(
            result(&compile("(defmacro empty [] 42)").unwrap()),
            "null\n"
        );
    }

    #[test]
    fn fixed_multiple_variadic_and_destructured_methods_use_hidden_abi() {
        let source = r#"
          (defmacro ^:private fixed "macro docs" {:marker 7} [[a b] {:keys [c]}]
            `(+ ~a ~b ~c))
          (defmacro methods ([x] x) ([x y & more] (last more)))
          (defmacro inspect []
            (let [m (get (:light-cljs/macros &env) 'cljs.user/fixed)]
              (list 'quote [(:doc m) (:marker m) (:private m) (:macro m)
                           (:arglists m) (:light-cljs/generation m)])))
          (and (= 42 (fixed [10 20] {:c 12}))
               (= [1 4] [(methods 1) (methods 1 2 3 4)])
               (= ["macro docs" 7 true true '([[a b] {:keys [c]}]) 1] (inspect)))"#;
        assert_eq!(
            result(&compile_with_macros(source, &mut javascript_host()).unwrap()),
            "true\n"
        );
        for invocation in ["(fixed)", "(fixed 1 2)", "(fixed 1 2 3 4)"] {
            let source = format!("(defmacro fixed ([x] x) ([x y z] z))\n{invocation}");
            let error = compile(&source).unwrap_err().to_string();
            assert!(
                error.contains("arity") && error.contains("definition"),
                "{error}"
            );
        }
    }

    #[test]
    fn source_order_preserves_expansions_and_macro_definition_generations() {
        let source = r#"
          (defmacro inner [] 1)
          (defmacro outer [] (inner))
          (defn before [] (inner))
          (defmacro inner [] 2)
          (defmacro nested [] '(inner))
          (= [1 1 2 2] [(before) (outer) (inner) (nested)])"#;
        assert_eq!(
            result(&compile_with_macros(source, &mut javascript_host()).unwrap()),
            "true\n"
        );
        for source in [
            "(future-macro) (defmacro future-macro [] 1)",
            "(if false (future-macro) 1) (defmacro future-macro [] 1)",
            "(defmacro early [] (future-macro)) (defmacro future-macro [] 1)",
        ] {
            assert!(compile(source).is_err(), "accepted forward macro: {source}");
        }
    }

    #[test]
    fn generated_declarations_and_top_level_do_interleave_in_source_order() {
        let source = r#"
          (defmacro install []
            '(do (defmacro answer [] 42) (def result (answer))))
          (do (install) (= [42 42] [result (answer)]))"#;
        assert_eq!(
            result(&compile_with_macros(source, &mut javascript_host()).unwrap()),
            "true\n"
        );
        assert_eq!(
            result(
                &compile(
                    "(defmacro install [x] '(defmacro generated [] 42)) (-> 1 install) (generated)"
                )
                .unwrap()
            ),
            "42\n"
        );
        for source in [
            "(if true (defmacro illegal [] 1) nil)",
            "(if false nil (defmacro illegal [] 1))",
            "(let [x 1] (defmacro illegal [] x))",
            "(fn [] (defmacro illegal [] 1))",
            "(defmacro install [] '(defmacro illegal [] 1)) (if true (install) nil)",
        ] {
            let error =
                analyze_with_macros(source, &parse(source).unwrap(), &mut javascript_host())
                    .unwrap_err();
            assert!(error.to_string().contains("top level"), "{source}: {error}");
            assert!(error.span.start < source.len());
        }
    }

    #[test]
    fn macro_and_value_binding_conflicts_include_declare_and_generated_definitions() {
        for source in [
            "(def m 1) (defmacro m [] 2)",
            "(declare m) (defmacro m [] 2)",
            "(def m) (defmacro m [] 2)",
            "(defmacro m [] 2) (def m 1)",
            "(defmacro m [] 2) (declare m)",
            "(defmacro m [] 2) (defn m [] 1)",
            "(defmacro m [] 2) (defmacro define [] '(def m 1)) (define)",
        ] {
            let error =
                analyze_with_macros(source, &parse(source).unwrap(), &mut javascript_host())
                    .unwrap_err();
            let message = error.to_string();
            assert!(
                message.contains("binding cljs.user/m") && message.contains("distinct names"),
                "{source}: {message}"
            );
            assert!(error.span.start < source.len());
        }
        assert_eq!(
            result(&compile("(declare value) (def value 42) value").unwrap()),
            "42\n"
        );
        // A visible ordinary binding must not silently fall back to the core
        // function/macro with the same spelling while helper demand is pending.
        for name in ["+", "when"] {
            let source = format!("(def {name} (fn [& xs] 99)) (defmacro m [] ({name} 1 2)) (m)");
            assert_eq!(result(&compile(&source).unwrap()), "99\n");
        }
    }

    #[test]
    fn local_calls_shadow_unqualified_macros_but_phase_misuses_are_located() {
        assert_eq!(
            result(
                &compile(
                    r#"
          (defmacro answer [] 42)
          (let [answer (fn [] 7)]
            (= [7 42] [(answer) (cljs.user/answer)]))"#
                )
                .unwrap()
            ),
            "true\n"
        );
        for use_form in ["m", "(apply m [])", "@m", "(set! m 1)"] {
            let source = format!("(defmacro m [] 42)\n{use_form}");
            let error =
                analyze_with_macros(&source, &parse(&source).unwrap(), &mut javascript_host())
                    .unwrap_err();
            assert!(
                error.to_string().contains("invoke it directly in source"),
                "{use_form}: {error}"
            );
            assert!(
                error.span.start >= source.find('\n').unwrap(),
                "{use_form}: {:?}",
                error.span
            );
        }
        // Source reflection exposes macro identity and metadata without a root.
        // Executing that Var still reports the macro/application phase error.
        assert_eq!(
            result(
                &compile(
                    r#"(defmacro m [] 42)
          (and (var? (var m)) (= #'m (resolve 'm)) (:macro (meta #'m))
               (try @#'m (catch :default e
                 (boolean (re-find #"compile-time macro" (.-message e)))))
               (try (#'m) (catch :default e
                 (boolean (re-find #"compile-time macro" (.-message e))))))"#
                )
                .unwrap()
            ),
            "true\n"
        );
    }

    #[test]
    fn namespace_and_cljc_macros_use_definition_site_names_and_selected_branch() {
        let sources = Sources::new();
        sources.write(
            "local/macros.cljc",
            r#"
          (ns local.macros)
          #?(:clj (defmacro ignored [] jvm-only)
             :cljs (defmacro ^:private inner [x] `(+ ~x 2)))
          (defmacro outer [x] (inner x))
          (defmacro nested [x] `(inner ~x))
          (let [inner (fn [x] 0)]
            (js/console.log (= [42 42 42] [(outer 40) (nested 40) (local.macros/inner 40)])))"#,
        );
        assert_eq!(
            execute(
                &sources
                    .compile_host("local.macros", &mut javascript_host())
                    .unwrap()
            ),
            "true\n"
        );
    }

    #[test]
    fn reused_host_refreshes_source_macros_and_preserves_configuration() {
        let mut host = javascript_host();
        host.set_assertions(false);
        host.limits.expansions = 64;
        host.evaluator_options.memory_limit = 64 * 1024 * 1024;
        let source = "(defmacro answer [] (assert false) 42) (answer)";
        for _ in 0..2 {
            assert_eq!(
                result(&compile_with_macros(source, &mut host).unwrap()),
                "42\n"
            );
            assert!(!host.assertions_enabled());
            assert_eq!(host.limits.expansions, 64);
            assert_eq!(host.evaluator_options.memory_limit, 64 * 1024 * 1024);
        }
        assert!(compile_with_macros("(answer)", &mut host).is_err());
        host.set_assertions(true);
        assert!(
            compile_with_macros(source, &mut host)
                .unwrap_err()
                .to_string()
                .contains("Assert failed")
        );
        assert_eq!(
            result(&compile_with_macros("(defmacro answer [] 7) (answer)", &mut host).unwrap()),
            "7\n"
        );
    }
}

mod reentrant_deferred_units {
    use super::*;

    fn checked(source: &str) -> String {
        let source = source.to_owned();
        let (send, receive) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut host = MacroHost::new();
            send.send(result(&compile_with_macros(&source, &mut host).unwrap()))
                .unwrap();
        });
        receive
            .recv_timeout(std::time::Duration::from_secs(20))
            .expect("reentrant compilation must not lock or hang")
    }

    #[test]
    fn nested_demands_expand_each_source_occurrence_once_and_share_ir() {
        assert_eq!(
            checked(
                r#"
          (def effects (atom []))
          (swap! effects conj :runtime-only)
          (def leaf (do (swap! effects conj :leaf-init) (fn [] 7)))
          (defmacro leaf-m [] (swap! effects conj :leaf-m) (leaf))
          (defn middle [] (leaf-m))
          (defmacro middle-m [] (swap! effects conj :middle-m) (middle))
          (defn outer [] (middle-m))
          (defmacro run [] (outer))
          (defmacro trace [] (list 'quote @effects))
          (= [7 7 [:middle-m :leaf-m :leaf-init] [:runtime-only :leaf-init] 7]
             [(run) (run) (trace) @effects (outer)])"#
            ),
            "true\n"
        );
    }

    #[test]
    fn snapshots_freeze_visibility_and_macro_generations_but_roots_are_late_bound() {
        assert_eq!(
            checked(
                r#"
          (def factor 2)
          (defmacro old [] 10)
          (defn helper [x] (+ (old) (* factor x)))
          (defmacro old [] 100)
          (def factor 3)
          (defmacro run [] (helper 4))
          (= [22 22] [(run) (helper 4)])"#
            ),
            "true\n"
        );
        for source in [
            "(defn helper [] future) (def future 1) (helper)",
            "(defn helper [] (if false future 1)) (def future 1) (helper)",
            "(defn helper [] (later)) (defmacro later [] 1) (helper)",
        ] {
            assert!(
                compile(source).is_err(),
                "accepted future binding: {source}"
            );
        }
        assert_eq!(
            checked(
                "(declare future) (defn helper [] future) (def future 42) (defmacro run [] (helper)) (run)"
            ),
            "42\n"
        );
    }

    #[test]
    fn analysis_cycles_are_distinct_from_ordinary_function_recursion() {
        let error = compile("(declare helper) (defmacro m [] (helper)) (defn helper [] (m)) (m)")
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("compile dependency cycle: cljs.user/helper -> cljs.user/helper"),
            "{error}"
        );
        assert_eq!(
            checked(
                "(declare f) (defn f [n] (if (zero? n) 42 (f (dec n)))) (defmacro m [] (f 4)) (m)"
            ),
            "42\n"
        );
    }

    #[test]
    fn definition_aliases_metadata_and_clean_session_options_are_frozen() {
        let sources = Sources::new();
        sources.write("dep/a.cljs", "(ns dep.a) (def value 40)");
        sources.write("dep/b.cljs", "(ns dep.b) (def value 100)");
        sources.write("owner.cljs", "(ns owner (:require [dep.a :as d])) (def ^{:doc \"original\"} value 2) (defn helper [] (+ d/value value)) (defmacro answer [] (helper))");
        sources.write(
            "app.cljs",
            "(ns app (:require [owner] [dep.b :as d])) (js/console.log (owner/answer) d/value)",
        );
        let mut host = MacroHost::new();
        assert_eq!(
            execute(&sources.compile_host("app", &mut host).unwrap()),
            "42 100\n"
        );
        sources.write("dep/a.cljs", "(ns dep.a) (def value 41)");
        assert_eq!(
            execute(&sources.compile_host("app", &mut host).unwrap()),
            "43 100\n"
        );
        for number in [42, 43] {
            sources.write("conditional.cljc", &format!("(ns conditional) (defn h [] #?(:cljs {number} :clj jvm-only)) (defmacro m [] (h)) (js/console.log (m))"));
            assert_eq!(
                execute(&sources.compile_host("conditional", &mut host).unwrap()),
                format!("{number}\n")
            );
        }
        let source = "(defn h [] (assert false) 42) (defmacro m [] (h)) (m)";
        host.set_assertions(false);
        assert_eq!(
            result(&compile_with_macros(source, &mut host).unwrap()),
            "42\n"
        );
        host.set_assertions(true);
        assert!(
            compile_with_macros(source, &mut host)
                .unwrap_err()
                .to_string()
                .contains("Assert failed")
        );
        assert_eq!(
            checked(
                "(def ^{:doc \"frozen\"} v 1) (defn h [] (:doc (meta #'v))) (defmacro m [] (h)) (= \"frozen\" (m))"
            ),
            "true\n"
        );
    }

    #[test]
    fn nested_execution_limit_is_terminal_and_a_clean_host_can_be_reused() {
        let mut host = MacroHost::new();
        host.evaluator_options.timeout = std::time::Duration::from_millis(300);
        let error = compile_with_macros(
            "(defn h [] (loop [] (recur))) (defmacro m [] (h)) (m)",
            &mut host,
        )
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("deadline") || error.contains("budget"),
            "{error}"
        );
        host.evaluator_options = Default::default();
        assert_eq!(
            result(
                &compile_with_macros("(defn h [] 42) (defmacro m [] (h)) (m)", &mut host).unwrap()
            ),
            "42\n"
        );
    }

    #[test]
    fn failed_nested_analysis_execution_and_conversion_do_not_poison_reused_host() {
        let mut host = MacroHost::new();
        for source in [
            "(defn h [] missing) (defmacro m [] (h)) (m)",
            "(defmacro broken [] (throw (js/Error. \"nested\"))) (defn h [] (broken)) (defmacro m [] (h)) (m)",
            "(def h (js* \"(() => {throw new Error('install')})()\")) (defmacro m [] h) (m)",
            "(defn h [] (js* \"not valid }\")) (defmacro m [] (h)) (m)",
            "(defn h [] (js/Promise.resolve 1)) (defmacro m [] (h)) (m)",
        ] {
            assert!(
                compile_with_macros(source, &mut host).is_err(),
                "accepted {source}"
            );
            assert_eq!(
                result(
                    &compile_with_macros("(defn h [] 42) (defmacro m [] (h)) (m)", &mut host)
                        .unwrap()
                ),
                "42\n"
            );
        }
    }
}

mod ordinary_helper_demand {
    use super::*;

    fn clean_host() -> MacroHost {
        MacroHost::new()
    }
    fn checked(source: &str) -> String {
        result(&compile_with_macros(source, &mut clean_host()).unwrap())
    }

    #[test]
    fn connection_is_application_only_and_initializer_order_is_preserved() {
        let js = compile_with_macros(
            r#"
          (def factor (+ 1 1))
          (defn twice [x] (* factor x))
          (def connection (js/connect))
          (defmacro doubled [x] (twice x))
          (pr-str [(doubled 21) (twice 21)])"#,
            &mut clean_host(),
        )
        .unwrap();
        assert_eq!(
            execute(&format!(
                "let count = 0; globalThis.connect = () => ++count; console.log({js}); console.log(count);"
            )),
            "[42 42]\n1\n"
        );
        let generated = compile_with_macros(r#"
          (defmacro install [] '(do (def factor 2) (defn twice [x] (* factor x)) (def connection (js/connect))))
          (install)
          (defmacro doubled [x] (twice x))
          (pr-str [(doubled 21) (twice 21)])"#, &mut clean_host()).unwrap();
        assert_eq!(
            execute(&format!(
                "let count = 0; globalThis.connect = () => ++count; console.log({generated}); console.log(count);"
            )),
            "[42 42]\n1\n"
        );
        assert_eq!(
            checked(
                r#"
          (def trace (atom []))
          (def factor (do (swap! trace conj :factor) 2))
          (defn twice [x] (* factor x))
          (def connection (do (swap! trace conj :connection) 1))
          (defmacro doubled [x] (twice x))
          (= [42 42 [:factor :connection]] [(doubled 21) (twice 21) @trace])"#
            ),
            "true\n"
        );
    }

    #[test]
    fn roots_are_fresh_on_reuse_and_standalone_mutation_is_never_replayed() {
        let mut host = clean_host();
        for _ in 0..2 {
            assert_eq!(
                result(
                    &compile_with_macros(
                        r#"
              (def hits (atom 0))
              (defn next-id [] (swap! hits inc))
              (defmacro id [] (next-id))
              (pr-str [(id) (id) @hits])"#,
                        &mut host
                    )
                    .unwrap()
                ),
                "[1 2 0]\n"
            );
            assert_eq!(
                result(
                    &compile_with_macros(
                        r#"
              (def hits (atom 0))
              (swap! hits inc)
              (def value 0)
              (set! value 1)
              (defmacro observed [] (+ @hits value))
              (= [0 1 1] [(observed) @hits value])"#,
                        &mut host
                    )
                    .unwrap()
                ),
                "true\n"
            );
        }
    }

    #[test]
    fn higher_order_collections_closures_apply_composition_and_branches_demand_values() {
        assert_eq!(
            checked(
                r#"
          (def factor 2)
          (def unused 0)
          (defn adder [x] (fn [y] (+ x y factor)))
          (def funcs [(adder 3)])
          (def mapping {:f (adder 4)})
          (defmacro run []
            (assert (= 10 (apply (first funcs) [5])))
            (assert (= 12 ((get mapping :f) 6)))
            (assert (= 22 ((comp (adder 3) (adder 4)) 11)))
            (assert (= 21 (reduce + (map (adder 3) [1 2 3]))))
            (if false unused (js/Math.imul 6 7)))
          (= 42 (run))"#
            ),
            "true\n"
        );
        let source =
            "(def unused (js/connect)) (defmacro m [] (if false unused (js/Math.imul 6 7))) (m)";
        assert!(compile_with_macros(source, &mut clean_host()).is_ok());
        let error = compile_with_macros(&source.replace("if false", "if true"), &mut clean_host())
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("connect") && error.contains("1:6"),
            "{error}"
        );
    }

    #[test]
    fn var_identity_quote_metadata_and_resolve_are_lazy_until_consumed() {
        // No console is installed in QuickJS. Var construction, equality,
        // metadata and resolve must not demand this pending host initializer.
        assert!(
            compile_with_macros(
                r#"
          (def ^{:doc "pending"} unused (js/connect))
          (declare missing)
          (defmacro m []
            (assert (var? (resolve 'missing)))
            (assert (= #'unused (resolve 'unused)))
            (assert (= "pending" (:doc (meta #'unused))))
            (assert (= 'unused 'unused))
            42)
          (m)"#,
                &mut clean_host()
            )
            .unwrap()
            .contains("42")
        );
        assert_eq!(
            checked(
                r#"
          (def hits (atom 0))
          (def value (do (swap! hits inc) 40))
          (defn f [x] (+ value x))
          (defmacro m []
            (assert (= 0 @hits))
            (assert (= 40 @#'value))
            (assert (= 1 @hits))
            (assert (= 42 (#'f 2)))
            (assert (= 43 ((resolve 'f) 3)))
            (assert (= 40 @(resolve 'value)))
            42)
          (= [42 1] [(m) @hits])"#
            ),
            "true\n"
        );
    }

    #[test]
    fn mutual_recursion_and_located_unavailable_and_initialization_cycles() {
        assert_eq!(
            checked(
                r#"
          (declare even-step odd-step)
          (defn even-step [n] (if (zero? n) true (odd-step (dec n))))
          (defn odd-step [n] (if (zero? n) false (even-step (dec n))))
          (defmacro m [] (even-step 20))
          (m)"#
            ),
            "true\n"
        );
        let error = compile_with_macros(
            "(declare future) (defmacro m [] future) (m) (def future 42)",
            &mut clean_host(),
        )
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("unavailable definition: cljs.user/future") && error.contains("1:10"),
            "{error}"
        );
        let error = compile_with_macros(
            "(declare a b) (def a b) (def b a) (defmacro m [] a) (m)",
            &mut clean_host(),
        )
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("root initialization cycle: cljs.user/a -> cljs.user/b -> cljs.user/a"),
            "{error}"
        );
    }

    #[test]
    fn failed_generation_retains_effects_error_and_defonce_until_redefined() {
        let source = r#"
          (def effects (atom 0))
          (def broken (do (swap! effects inc) (throw (js/Error. "sticky"))))
          (defmacro probe []
            (let [a (try broken (catch :default e e))
                  b (try broken (catch :default e e))]
              (assert (identical? a b))
              (assert (= 1 @effects)) 1))
          (probe)
          (defonce broken 7)
          (probe)
          (def broken 42)
          (defmacro repair [] (assert (= 42 broken)) (assert (= 1 @effects)) 42)
          (repair)"#;
        // The emitted application's earlier throwing initializer is intentional;
        // assertions execute in the compiler and prove the failure state policy.
        assert!(compile_with_macros(source, &mut clean_host()).is_ok());
    }

    #[test]
    fn defonce_preserves_pending_nil_false_undefined_and_allows_declared_slots() {
        assert_eq!(
            checked(
                r#"
          (def effects (atom 0))
          (def pending (do (swap! effects inc) 42))
          (defonce pending 99)
          (def n nil) (defonce n 99)
          (def f false) (defonce f 99)
          (def u js/undefined) (defonce u 99)
          (declare declared) (defonce declared 7)
          (defmacro m []
            (assert (= 0 @effects))
            (assert (= 42 pending))
            (assert (= 1 @effects))
            (assert (nil? n)) (assert (false? f))
            (assert (identical? js/undefined u))
            (assert (not (exists? u)))
            (assert (var? (resolve 'u)))
            (assert (= 7 declared)) 42)
          (= [42 42 nil false 99 7 1] [(m) pending n f u declared @effects])"#
            ),
            "true\n"
        );
    }

    #[test]
    fn saved_function_code_captures_and_active_global_generations() {
        assert_eq!(
            checked(
                r#"
          (def factor 2)
          (defn f [x] (+ factor x))
          (def saved f)
          (defmacro before [] (saved 3))
          (before)
          (def first-result 5)
          (def factor 10)
          (defn f [x] (* factor x))
          (defmacro after [] (assert (= 13 (saved 3))) (assert (= 30 (f 3))) 13)
          (= [5 13 13 30] [first-result (after) (saved 3) (f 3)])"#
            ),
            "true\n"
        );
    }

    #[test]
    fn scopes_preserve_pending_failed_roots_parallel_initializers_and_overlay_set() {
        assert!(compile_with_macros(r#"
          (def effects (atom 0))
          (def ^:dynamic x (do (swap! effects inc) 1))
          (def y 2)
          (def unavailable (js/connect))
          (def broken (do (swap! effects inc) (throw (js/Error. "sticky"))))
          (def bad-entry (do (swap! effects inc) (throw (js/Error. "entry"))))
          (defmacro m []
            (assert (= 9 (with-redefs [unavailable 9] unavailable)))
            (assert (= 0 @effects))
            (assert (= 4 (binding [x 3] (set! x 4) x)))
            (assert (= 0 @effects))
            (try (binding [x 5] (binding [x 6] (throw (js/Error. "pending")))) (catch :default e nil))
            (assert (= 0 @effects))
            (assert (= [2 1] (with-redefs [x y y x] [x y])))
            (assert (= 1 @effects))
            (try (binding [x 5] (binding [x 6] (throw (js/Error. "body")))) (catch :default e nil))
            (assert (= 1 x))
            (try broken (catch :default e nil))
            (assert (= 2 @effects))
            (assert (= 8 (with-redefs [broken 8] broken)))
            (try (with-redefs [broken 8] (with-redefs [broken 9] (throw (js/Error. "nested")))) (catch :default e nil))
            (try broken (catch :default e nil))
            (assert (= 2 @effects))
            (try (with-redefs [x 10 y broken] 0) (catch :default e nil))
            (assert (= [1 2] [x y]))
            (try (with-redefs [x 10 y bad-entry] 0) (catch :default e nil))
            (assert (= [1 2] [x y]))
            (assert (= 3 @effects))
            (try bad-entry (catch :default e nil))
            (assert (= 3 @effects))
            (set! y 12)
            (assert (= 12 y))
            42)
          (m)"#, &mut clean_host()).is_ok());
    }

    #[test]
    fn executed_nested_definitions_change_only_compilation_roots() {
        assert_eq!(
            checked(
                r#"
          (def x 1)
          (defn update [] (def x 7) x)
          (defn object [] (def x (js* "{answer: 42}")) (.-answer x))
          (defn scoped [] (binding [x 3] (def x 4) x))
          (defmacro m [] (assert (= 4 (scoped))) (assert (= 1 x)) (assert (= 42 (object))) (assert (= 7 (update))) (assert (= 7 x)) 7)
          (= [7 1 4 1 7 7] [(m) x (scoped) x (update) x])"#
            ),
            "true\n"
        );
    }
}

mod declaration_bundle_demand {
    use super::*;
    fn checked(source: &str) -> String {
        let mut host = MacroHost::new();
        result(&compile_with_macros(source, &mut host).unwrap())
    }
    #[test]
    fn record_type_protocols_and_core_operations_share_lowering_with_separate_state() {
        assert_eq!(
            checked(
                r#"
          (def hits (atom 0))
          (defprotocol Own (own [x]))
          (defprotocol External (external [x]))
          (defrecord R [x] Own (own [_] (swap! hits inc) x))
          (deftype T [x] Own (own [this] (if (zero? x) 0 (own (->T (dec x)))))
            ICounted (-count [_] x) ILookup (-lookup [_ k] (if (= k :x) x nil))
            IHash (-hash [_] x) IMeta (-meta [_] {:x x}))
          (extend-type R External (external [this] (+ 10 (:x this))))
          (defn helper [] (let [r (map->R {:x 7}) t (->T 3) saved own]
            [(saved r) (external r) (own t) (count t) (get t :x) (hash t)
             (:x (meta t)) (count r) (:x (meta (with-meta r {:x 9}))) @hits]))
          (defmacro m [] (list 'quote (helper)))
          (= [[7 17 0 3 3 3 3 1 9 1] [7 17 0 3 3 3 3 1 9 1] 1]
             [(m) (helper) @hits])"#
            ),
            "true\n"
        );
    }
    #[test]
    fn native_marker_higher_order_extend_protocol_and_replacement_follow_source_frontier() {
        assert_eq!(
            checked(
                r#"
          (defprotocol Marker)
          (defprotocol P (f [x]))
          (extend-type nil Marker)
          (extend-protocol P nil (f [_] :nil) string (f [x] (str x "!")) array (f [x] (alength x)) default (f [_] :default))
          (defn helper [] [(satisfies? Marker nil) (mapv f [nil "s" #js [1 2] true])])
          (defmacro before [] (list 'quote (helper)))
          (def first-result (before))
          (defmacro freeze [] first-result) (freeze)
          (extend-type string P (f [_] :new))
          (defmacro after [] (list 'quote (helper)))
          (= [[true [:nil "s!" 2 :default]] [true [:nil :new 2 :default]] [true [:nil :new 2 :default]]]
             [first-result (after) (helper)])"#
            ),
            "true\n"
        );
    }
    #[test]
    fn computed_targets_are_protocol_specific_and_initialized_once() {
        let mut host = MacroHost::new();
        let js = compile_with_macros(
            r#"
          (def hits (atom 0))
          (defprotocol P (f [x])) (defprotocol Unrelated (other [x]))
          (deftype T [])
          (def target (do (swap! hits inc) T))
          (def unavailable (js/connect))
          (extend-type unavailable Unrelated (other [_] 99))
          (extend-type target P (f [_] 42))
          (defn helper [] [(f (->T)) (f (->T)) @hits])
          (defmacro m [] (list 'quote (helper)))
          (pr-str [(m) (helper) @hits])"#,
            &mut host,
        )
        .unwrap();
        // Runtime executes the standalone declarations in order, so its host
        // effect needs to exist even though compilation never demanded it.
        assert_eq!(
            execute(&format!(
                "globalThis.connect = () => function Unavailable() {{}}; console.log({js});"
            )),
            "[[42 42 1] [42 42 1] 1]\n"
        );
    }
    #[test]
    fn reify_captures_and_executed_specify_use_canonical_expression_semantics() {
        assert_eq!(
            checked(
                r#"
          (defprotocol P (f [x]))
          (def standalone #js {})
          (specify! standalone P (f [_] :runtime))
          (defn helper [n] (let [r (reify P (f [_] n)) obj #js {}]
            (specify! obj P (f [_] (+ n 1))) [(f r) (f obj) (satisfies? P standalone)]))
          (defmacro m [] (list 'quote (helper 7)))
          (= [[7 8 false] [7 8 true]] [(m) (helper 7)])"#
            ),
            "true\n"
        );
    }
    #[test]
    fn failed_bundle_is_sticky_and_fresh_compilation_repairs_it() {
        let mut host = MacroHost::new();
        let js = compile_with_macros(
            r#"
          (def hits (atom 0)) (defprotocol P (f [x]))
          (def bad (do (swap! hits inc) (throw (js/Error. "bundle-boom"))))
          (extend-protocol P string (f [_] 1) bad (f [_] 2))
          (defmacro m []
            (let [a (try (f "s") (catch :default e (.-message e)))
                  b (try (f "s") (catch :default e (.-message e)))]
              (assert (= a b)) (assert (= 1 @hits))
              (assert (nil? (aget f "string")))
              (assert (nil? (aget P "string"))) 42))
          (m)"#,
            &mut host,
        )
        .unwrap();
        assert!(js.contains("return 42"));
        assert_eq!(result(&compile_with_macros("(defprotocol P (f [x])) (extend-type string P (f [_] 42)) (defmacro m [] (f \"s\")) (m)", &mut host).unwrap()), "42\n");
    }
    #[test]
    fn bundle_cycles_report_paths_and_declaration_products_support_redefinition() {
        let mut host = MacroHost::new();
        let error = compile_with_macros(
            r#"
          (defprotocol P (f [x])) (declare target)
          (extend-type target P (f [_] 7))
          (def target (do (f "s") js/String))
          (defmacro m [] (f "s")) (m)"#,
            &mut host,
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("bundle initialization cycle"), "{error}");
        assert!(error.contains("cljs.user/target"), "{error}");
        assert_eq!(
            checked(
                r#"
          (deftype T [x]) (def T 17)
          (defmacro first [] T) (def a (first))
          (defmacro freeze [] a) (freeze)
          (deftype T [x] Object (value [_] x))
          (extend-type T Object (extra [_] 9))
          (defmacro second [] [(.value (->T 7)) (.extra (->T 7))])
          (= [17 [7 9] 9] [a (second) (.extra (->T 7))])"#
            ),
            "true\n"
        );
    }
}

#[test]
fn deferred_declaration_methods_expand_once_at_the_definition_site() {
    let mut host = MacroHost::new();
    let js = compile_with_macros(
        r#"
      (def hits (atom 0))
      (defprotocol P (f [x]))
      (defmacro value [] (swap! hits inc) 7)
      (deftype T [] P (f [_] (value)))
      (defmacro value [] 99)
      (defmacro run [] (let [v (->T)] (assert (= 7 (f v))) (assert (= 1 @hits)) (f v)))
      (= [7 7 0] [(run) (f (->T)) @hits])"#,
        &mut host,
    )
    .unwrap();
    assert_eq!(result(&js), "true\n");
}

#[test]
fn failed_extension_generation_can_be_replaced_without_repeating_host_effects() {
    let mut host = MacroHost::new();
    let js = compile_with_macros(
        r#"
      (def hits (atom 0)) (defprotocol P (f [x]))
      (def target (do (swap! hits inc) (throw (js/Error. "bad target"))))
      (extend-type target P (f [_] 1))
      (defmacro fail [] (try (f "s") (catch :default e nil)) (assert (= 1 @hits)) nil)
      (fail) (def target js/String)
      (extend-type target P (f [_] 42))
      (defmacro repaired [] (assert (= 1 @hits)) (f "s")) (repaired)"#,
        &mut host,
    )
    .unwrap();
    assert!(js.contains("return 42"));
}

#[test]
fn delayed_macro_expansion_excludes_future_extensions_even_after_new_dispatch_is_ready() {
    let mut host = MacroHost::new();
    let js = compile_with_macros(
        r#"
      (defprotocol P (f [x]))
      (extend-type string P (f [_] 1))
      (defmacro snapshot [] (f "s"))
      (def earlier (snapshot))
      (extend-type string P (f [_] 2))
      (snapshot)
      (defmacro demand-earlier [] earlier)
      (= [1 2 2] [(demand-earlier) (snapshot) (f "s")])"#,
        &mut host,
    )
    .unwrap();
    assert_eq!(result(&js), "true\n");
}

#[test]
fn core_extension_queries_and_saved_protocol_methods_demand_visible_candidates() {
    let mut host = MacroHost::new();
    let js = compile_with_macros(
        r#"
      (def hits (atom 0)) (defprotocol P (f [x])) (deftype T [])
      (def target (do (swap! hits inc) T))
      (extend-type target P (f [_] 7) ICounted (-count [_] 9))
      (def saved f)
      (defmacro m [] (let [v (->T)] (assert (= 9 (count v)))
        (assert (= 1 @hits)) [(saved v) (apply saved [v])]))
      (= [[7 7] 9 1] [(m) (count (->T)) @hits])"#,
        &mut host,
    )
    .unwrap();
    assert_eq!(result(&js), "true\n");
}

#[test]
fn restoring_a_dispatch_frontier_preserves_executed_helper_mutation() {
    let mut host = MacroHost::new();
    let js = compile_with_macros(
        r#"
      (defprotocol P (f [x])) (extend-type string P (f [_] 1))
      (defmacro snapshot [] (f "s")) (def earlier (snapshot))
      (extend-type string P (f [_] 2))
      (defmacro mutate [] (f "s") (aset f "string" (fn [_] 7)) nil) (mutate)
      (defmacro demand-earlier [] earlier)
      (= [1 7 2] [(demand-earlier) (snapshot) (f "s")])"#,
        &mut host,
    )
    .unwrap();
    assert_eq!(result(&js), "true\n");
}

#[test]
fn ordinary_dual_use_require_executes_exact_tools_example_through_api_and_cli() {
    let s = Sources::new();
    s.write("tools.cljc", "(ns tools) #?(:cljs (def factor 2) :clj (def factor jvm-only-placeholder)) (defn twice [x] (* factor x)) (defmacro doubled [x] (twice x))");
    s.write("app.cljs", "(ns app (:require [tools :as t :refer [doubled]])) [(doubled 21) (t/doubled 21) (t/twice 21)]");
    let mut h = MacroHost::new();
    let inspect = |js: &str| execute(&format!("const value = {js}; console.log(String(value));"));
    for _ in 0..2 {
        for host in [&mut h, &mut MacroHost::new()] {
            let js = s.compile_host("app", host).unwrap();
            assert_eq!(inspect(&js), "[42 42 42]\n");
            let evaluator = light_cljs::evaluator::Evaluator::new(Default::default()).unwrap();
            let value = evaluator.evaluate_js(&format!("String({js})")).unwrap();
            assert_eq!(evaluator.print(value).unwrap(), "\"[42 42 42]\"");
        }
    }
    let output = Command::new(env!("CARGO_BIN_EXE_light-cljs"))
        .args(["--compile", "--entry", "app", "--source-root"])
        .arg(&s.0)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        inspect(&String::from_utf8(output.stdout).unwrap()),
        "[42 42 42]\n"
    );
}

#[test]
fn ordinary_requires_share_transitive_helpers_preserve_effects_privacy_and_shadowing() {
    let s = Sources::new();
    let mut h = MacroHost::new();
    s.write("leaf.cljc", "(ns leaf) #?(:cljs (def factor 2) :clj (require 'jvm.missing)) (def hits (atom 0)) (swap! hits inc) (defn twice [x] (* factor x)) (js/console.log \"leaf\")");
    s.write("left.cljs", "(ns left (:require [leaf :as l])) (defn higher [f x] (apply f [x])) (defn helper [x] (higher l/twice x)) (defmacro doubled [x] (helper x)) (js/console.log \"left\")");
    s.write("right.cljs", "(ns right (:require [leaf :as l])) (defn ^:private secret [x] (l/twice x)) (defmacro exposed [x] (secret x)) (js/console.log \"right\")");
    s.write("app.cljs", "(ns app (:refer-clojure :exclude [when]) (:require [left :as l :refer [doubled] :rename {doubled when}] [right :as r] [leaf :as leaf])) (js/console.log (when 21) (l/doubled 21) (r/exposed 21) (let [when (fn [x] x)] (when 7)) @leaf/hits)");
    for _ in 0..2 {
        assert_eq!(
            execute(&s.compile_host("app", &mut h).unwrap()),
            "leaf\nleft\nright\n42 42 42 7 1\n"
        );
    }
    s.write("app.cljs", "(ns app (:require [right :refer [secret]])) 0");
    let e = s.compile_host("app", &mut h).unwrap_err();
    assert!(e.message.contains("private"), "{e}");
    assert!(e.path.unwrap().ends_with("app.cljs"));
    s.write(
        "app.cljs",
        "(ns app (:require [right :as r])) (r/exposed 21)",
    );
    assert_eq!(
        result(&s.compile_host("app", &mut h).unwrap()),
        "leaf\nright\n42\n"
    );
}

#[test]
fn required_unrelated_initializers_run_only_in_application_and_failure_is_not_swallowed() {
    let s = Sources::new();
    s.write(
        "tools.cljs",
        "(ns tools) (def unavailable (js/connect)) (defmacro answer [] 42) (js/startup)",
    );
    s.write(
        "app.cljs",
        "(ns app (:require [tools :refer [answer]])) (answer)",
    );
    let js = s.compile("app").unwrap();
    assert_eq!(
        execute(&format!(
            "globalThis.connect = () => {{console.log('connect'); return 1;}}; globalThis.startup = () => console.log('startup'); console.log({js});"
        )),
        "connect\nstartup\n42\n"
    );
    s.write(
        "tools.cljs",
        "(ns tools) (def unavailable (throw (js/Error. \"runtime-only\"))) (defmacro answer [] 42)",
    );
    let js = s.compile("app").unwrap();
    let output = Command::new("node").args(["--eval", &js]).output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("runtime-only"));
}

#[test]
fn macro_source_selection_splices_discards_conflicts_and_recovery_are_current() {
    let s = Sources::new();
    let mut h = MacroHost::new();
    s.write("leaf.cljs", "(ns leaf) (defn helper [x] (+ x 2))");
    s.write("tools.clj", "invalid ignored companion");
    s.write("tools.cljc", "(ns tools #?@(:cljs [(:require [leaf :as h])] :clj [(:require missing.jvm)])) #_#?(:clj #unknown/tag nil :cljs nil) (do #?@(:cljs [(defmacro value [x] (h/helper x))] :clj [(defmacro value [] jvm-only)]))");
    s.write("app.cljc", "(ns app #?(:cljs (:require [tools :refer [value]]) :clj (:require missing.jvm))) #?(:default (value 40) :cljs 99)");
    assert_eq!(result(&s.compile_host("app", &mut h).unwrap()), "42\n");
    s.write("leaf.cljs", "(ns leaf) (defn helper [x] (+ x 3))");
    assert_eq!(result(&s.compile_host("app", &mut h).unwrap()), "43\n");
    s.write(
        "leaf.cljs",
        "(ns leaf) (defn helper [x] (throw (js/Error. \"helper-broken\")))",
    );
    let e = s.compile_host("app", &mut h).unwrap_err();
    for detail in ["helper-broken", "tools.cljc", "app.cljc"] {
        assert!(e.to_string().contains(detail), "missing {detail}: {e}");
    }
    s.write("leaf.cljs", "(ns leaf) (defn helper [x] (+ x 4))");
    assert_eq!(result(&s.compile_host("app", &mut h).unwrap()), "44\n");
    s.write("tools.cljs", "(ns tools) (defmacro value [x] 45)");
    assert_eq!(result(&s.compile_host("app", &mut h).unwrap()), "45\n");
    fs::remove_file(s.0.join("tools.cljs")).unwrap();
    s.write(
        "tools.cljc",
        "(ns tools (:require [missing :as h])) (defmacro value [x] (h/helper x))",
    );
    let e = s.compile_host("app", &mut h).unwrap_err();
    assert!(e.message.contains("not found"), "{e}");
    assert!(e.path.unwrap().ends_with("tools.cljc"));
    s.write(
        "tools.cljc",
        "(ns tools (:require [leaf :as h])) (defmacro value [x] (h/helper x))",
    );
    assert_eq!(result(&s.compile_host("app", &mut h).unwrap()), "44\n");
    s.write(
        "app.cljs",
        "(ns app) #_nil (require '[tools :refer [value]]) (value 40)",
    );
    assert_eq!(result(&s.compile_host("app", &mut h).unwrap()), "44\n");
    s.write(
        "tools.cljc",
        "(ns tools (:require app)) (defmacro value [x] x)",
    );
    let e = s.compile_host("app", &mut h).unwrap_err();
    assert!(e.message.contains("app -> tools -> app"), "{e}");
    s.write("tools.cljc", "(ns tools) (defmacro value [x] 44)");
    assert_eq!(result(&s.compile_host("app", &mut h).unwrap()), "44\n");
    let duplicate = Sources::new();
    duplicate.write("tools.cljc", "(ns tools) (defmacro value [x] 999)");
    let roots = [s.0.clone(), duplicate.0.clone()];
    let e = compile_namespace_with_macros("app", &roots, &mut h).unwrap_err();
    assert!(e.message.contains("ambiguous namespace tools"), "{e}");
    assert!(e.path.unwrap().ends_with("app.cljs"));
    duplicate.write("tools.cljs", "(ns tools) (defmacro value [x] 46)");
    assert_eq!(
        result(&compile_namespace_with_macros("app", &roots, &mut h).unwrap()),
        "46\n"
    );
    s.write("tools.cljs", "(ns tools) (defmacro value [x] 47)");
    assert!(
        compile_namespace_with_macros("app", &roots, &mut h)
            .unwrap_err()
            .message
            .contains("ambiguous namespace tools")
    );
    fs::remove_file(s.0.join("tools.cljs")).unwrap();
    assert_eq!(
        result(&compile_namespace_with_macros("app", &roots, &mut h).unwrap()),
        "46\n"
    );
}

#[test]
fn imported_macro_values_and_private_macros_remain_phase_errors() {
    let s = Sources::new();
    let mut h = MacroHost::new();
    s.write(
        "tools.cljs",
        "(ns tools) (defmacro ^:private secret [] 42) (defmacro public [] (secret))",
    );
    for expression in [
        "secret",
        "(t/secret)",
        "public",
        "t/public",
        "(apply public [])",
    ] {
        s.write(
            "app.cljs",
            &format!("(ns app (:require [tools :as t :refer [public]])) {expression}"),
        );
        let e = s.compile_host("app", &mut h).unwrap_err();
        assert!(
            e.message.contains("private")
                || e.message.contains("macro")
                || e.message.contains("unresolved"),
            "{expression}: {e}"
        );
    }
    s.write("app.cljs", "(ns app (:require [tools :refer [secret]])) 0");
    assert!(
        s.compile_host("app", &mut h)
            .unwrap_err()
            .message
            .contains("private")
    );
    s.write(
        "app.cljs",
        r#"(ns app (:require [tools :as t :refer [public]]))
      (and (var? #'public) (= #'public (resolve 'public)) (:macro (meta #'t/public))
           (try (#'public) (catch :default e
             (boolean (re-find #"compile-time macro" (.-message e))))))"#,
    );
    assert_eq!(result(&s.compile_host("app", &mut h).unwrap()), "true\n");
    s.write(
        "app.cljs",
        "(ns app (:require [tools :as t :refer [public]])) [(public) (t/public)]",
    );
    assert_eq!(
        execute(&format!(
            "console.log(String({}));",
            s.compile_host("app", &mut h).unwrap()
        )),
        "[42 42]\n"
    );
}

mod top_level_definition_groups {
    use super::*;

    fn checked(source: &str, host: &mut MacroHost) {
        let js = compile_with_macros(source, host).unwrap();
        assert_eq!(result(&js), "true\n");
        let evaluator = light_cljs::evaluator::Evaluator::new(Default::default()).unwrap();
        let value = evaluator.evaluate_js(&format!("Number({js})")).unwrap();
        assert_eq!(evaluator.number(value).unwrap(), 1.0);
    }

    #[test]
    fn exists_observes_unavailable_roots_values_and_conditional_products() {
        let mut host = MacroHost::new();
        for _ in 0..2 {
            checked(
                r#"
              (def trace (atom []))
              (declare unavailable)
              (def value (do (swap! trace conj :value) 42))
              (def nil-value nil) (def false-value false) (def undefined-value js/undefined)
              (def ^:dynamic bound js/undefined)
              (if false (def omitted 1) nil)
              (let [] (def installed 7) (def own-check (exists? installed)))
              (def old 9) (if false (def old 10) nil)
              (defn probe [demand]
                (assert (= (if demand [] [:value]) @trace))
                (assert (not (exists? unavailable)))
                (assert (not (exists? unknown)))
                (assert (not (exists? made.up/unknown)))
                (assert (not (exists? omitted)))
                (assert (exists? cljs.core/mapv))
                (assert (exists? cljs.user/value))
                (assert (exists? value))
                (assert (= [:value] @trace))
                (assert (exists? nil-value)) (assert (exists? false-value))
                (assert (not (exists? undefined-value)))
                (assert (exists? installed)) (assert own-check) (assert (exists? old))
                (assert (not (exists? bound)))
                (binding [bound false] (assert (exists? bound)))
                (assert (not (exists? bound)))
                (let [unavailable nil value js/undefined]
                  (assert (exists? unavailable)) (assert (not (exists? value))))
                (assert (not (exists? js/noSuchGlobal.child)))
                (assert (exists? js/Math.imul))
                (let [o #js {:nested #js {:x false}}]
                  (assert (exists? (.-x (.-nested (do (swap! trace conj :nested) o)))))
                  (assert (not (exists? (.-missing o)))))
                (assert (exists? (do (swap! trace conj :expression) false)))
                (assert (not (exists? (do (swap! trace conj :missing) js/undefined))))
                (assert (= [:value :nested :expression :missing] @trace)) true)
              (defmacro m [] (probe true))
              (and (m) (= [:value] @trace) (probe false))
            "#,
                &mut host,
            );
        }
    }

    #[test]
    fn exists_preserves_initializer_errors_cycles_visibility_and_reuse() {
        let mut host = MacroHost::new();
        for (source, expected) in [
            (
                "(declare absent) (defmacro m [] absent) (m)",
                "unavailable definition: cljs.user/absent",
            ),
            (
                "(def broken (throw (js/Error. \"exists-failure\"))) (defmacro m [] (exists? broken)) (m)",
                "exists-failure",
            ),
            (
                "(declare a b) (def a b) (def b a) (defmacro m [] (exists? a)) (m)",
                "root initialization cycle: cljs.user/a -> cljs.user/b -> cljs.user/a",
            ),
            (
                "(let [] (def x (exists? y)) (def y 1)) (defmacro m [] x) (m)",
                "bundle initialization cycle",
            ),
            (
                "(defmacro m [] (exists? future)) (assert (not (m))) (def future 1) (defmacro after [] (exists? future)) (assert (after)) true",
                "",
            ),
        ] {
            if expected.is_empty() {
                checked(source, &mut host);
            } else {
                let error = compile_with_macros(source, &mut host)
                    .unwrap_err()
                    .to_string();
                assert!(error.contains(expected) && error.contains("1:"), "{error}");
            }
            checked(
                "(defmacro clean [] (not (exists? absent))) (clean)",
                &mut host,
            );
        }
    }

    #[test]
    fn complete_owners_preserve_captures_order_once_and_phase_roots() {
        let mut host = MacroHost::new();
        for _ in 0..2 {
            checked(
                r#"
              (def trace (atom []))
              (let [x (do (swap! trace conj :capture) 1)]
                (def a x)
                (let [y (+ a 1)] (defn bar [] x) (defn baz [] y))
                (swap! trace conj :installed))
              (defmacro observe []
                (assert (= [1 2 1] [(bar) (baz) a]))
                (assert (= [:capture :installed] @trace))
                (assert (= 1 (bar))) true)
              (and (observe) (observe) (= [1 2 1 [:capture :installed]] [(bar) (baz) a @trace]))
            "#,
                &mut host,
            );
        }
    }

    #[test]
    fn selected_groups_and_conditional_products_keep_lazy_availability() {
        let mut host = MacroHost::new();
        assert!(
            compile_with_macros(
                r#"
          (let [connection (js/connect)] (def unavailable connection))
          (defmacro skip [] (if false unavailable 42)) (skip)
        "#,
                &mut host
            )
            .is_ok()
        );
        let error = compile_with_macros(
            r#"
          (let [connection (js/connect)] (def unavailable connection))
          (defmacro select [] unavailable) (select)
        "#,
            &mut host,
        )
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("connect") && error.contains("2:11"),
            "{error}"
        );
        checked(
            r#"
          (def preserved 7)
          (if false (def preserved 99) (def present 42))
          (defmacro m [] (assert (= 7 preserved)) (assert (= 42 present)) true)
          (and (m) (= [7 42] [preserved present]))
        "#,
            &mut host,
        );
        let error = compile_with_macros(
            "(if false (def absent 1) nil) (defmacro m [] absent) (m)",
            &mut host,
        )
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("unavailable definition: cljs.user/absent") && error.contains("1:1"),
            "{error}"
        );
        assert!(
            compile_with_macros(
                r#"
          (def pending (js/connect))
          (let [] (defonce pending 99) (def selected 42))
          (defmacro m [] selected) (m)
        "#,
                &mut host
            )
            .is_ok()
        );
        checked(
            r#"
          (def pending 7)
          (let [] (defonce pending 99) (def selected (+ pending 1)))
          (defmacro m [] (assert (= [7 8] [pending selected])) true)
          (and (m) (= [7 8] [pending selected]))
        "#,
            &mut host,
        );
    }

    #[test]
    fn own_initializers_observe_prior_generations_and_defonce_checks_conditional_installation() {
        checked(
            r#"
          (def x 1)
          (let [y x] (def x (+ y 1)))
          (if (= x 2) (def x 3) nil)
          (if false (def absent 1) nil)
          (let [] (defonce absent 2) (def y absent))
          (if false (def other 1) nil)
          (defonce other 8)
          (defmacro m [] (assert (= [3 2 2 8] [x absent y other])) true)
          (and (m) (= [3 2 2 8] [x absent y other]))
        "#,
            &mut MacroHost::new(),
        );
    }

    #[test]
    fn executed_bindings_restore_overlays_without_installing_temporary_roots() {
        checked(
            r#"
          (def ^:dynamic x 1)
          (binding [x 5] (def x 2) (def y x))
          (let [] (binding [x 5] (def x 3)) (def z x))
          (let [] (def mutable 1) (set! mutable 2) (def after-set mutable))
          (defmacro m [] (assert (= [1 2 1 2 2] [x y z mutable after-set])) true)
          (and (m) (= [1 2 1 2 2] [x y z mutable after-set]))
        "#,
            &mut MacroHost::new(),
        );
    }

    #[test]
    fn generations_and_macro_expansion_are_source_ordered_without_replay() {
        checked(
            r#"
          (def expansions (atom 0))
          (defmacro value [] (swap! expansions inc))
          (let [x (value)] (defn f [] x))
          (defmacro before [] (assert (= 1 @expansions)) (f))
          (def first-value (before))
          (defmacro freeze [] first-value) (freeze)
          (let [x (value)] (defn f [] x) (defn g [] (f)))
          (defmacro after [] (assert (= 2 @expansions)) (assert (= 2 (g))) (f))
          (= [1 2 2 0] [first-value (after) (g) @expansions])
        "#,
            &mut MacroHost::new(),
        );
    }

    #[test]
    fn earlier_products_and_recursive_functions_do_not_hide_real_cycles() {
        checked(
            r#"
          (declare even-step odd-step)
          (let [zero 0]
            (defn even-step [n] (if (= zero n) true (odd-step (dec n))))
            (defn odd-step [n] (if (= zero n) false (even-step (dec n))))
            (def ready (even-step 12)))
          (defmacro m [] ready)
          (and (m) ready (odd-step 11))
        "#,
            &mut MacroHost::new(),
        );
        for source in [
            "(declare a b) (let [] (def a b) (def b a)) (defmacro m [] a) (m)",
            "(declare a b) (let [] (def a b)) (let [] (def b a)) (defmacro m [] a) (m)",
        ] {
            let error = compile_with_macros(source, &mut MacroHost::new())
                .unwrap_err()
                .to_string();
            assert!(
                error.contains("bundle initialization cycle") && error.contains("1:"),
                "{error}"
            );
        }
    }

    #[test]
    fn throwing_owner_is_sticky_and_fresh_or_reused_hosts_recover() {
        let mut host = MacroHost::new();
        for _ in 0..2 {
            assert!(
                compile_with_macros(
                    r#"
              (def effects (atom 0))
              (let [] (def a (swap! effects inc)) (throw (js/Error. "owner failed")) (def b 2))
              (defmacro m []
                (let [one (try a (catch :default e e)) two (try b (catch :default e e))]
                  (assert (identical? one two)) (assert (= 1 @effects))) true)
              (m)
              (let [] (def a 42) (def b a))
              (defmacro repaired [] (assert (= [42 42 1] [a b @effects])) true) (repaired)
            "#,
                    &mut host
                )
                .is_ok()
            );
            let error = compile_with_macros(
                "(let [] (def a (throw (js/Error. \"located\")))) (defmacro m [] a) (m)",
                &mut host,
            )
            .unwrap_err()
            .to_string();
            assert!(
                error.contains("located") && error.contains("1:1"),
                "{error}"
            );
            checked(
                "(let [x 42] (defn a [] x)) (defmacro m [] (= 42 (a))) (m)",
                &mut host,
            );
        }
    }

    #[test]
    fn terminal_limit_in_owner_cannot_be_caught_and_host_recovers() {
        let mut host = MacroHost::new();
        host.evaluator_options.timeout = std::time::Duration::from_millis(300);
        let error = compile_with_macros(
            r#"
          (let [] (def never (loop [] (recur))))
          (defmacro m [] (try never (catch :default e 42))) (m)
        "#,
            &mut host,
        )
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("deadline") || error.contains("budget"),
            "{error}"
        );
        host.evaluator_options = Default::default();
        checked(
            "(let [x 42] (defn a [] x)) (defmacro m [] (= 42 (a))) (m)",
            &mut host,
        );
    }
}

#[test]
fn demanded_record_initializers_restore_generation_views_after_nested_demand_and_failure() {
    let mut host = MacroHost::new();
    let source = r#"
      (def hits (atom 0))
      (defprotocol P (p [x]))
      (defrecord R [x] P (p [_] x))
      (def old (->R 3))
      (declare current)
      (def nested [(p old) (p current) (p (->R 4))])
      (defn live [] (->R 2))
      (def broken (do (swap! hits inc)
                      (assert (= 5 (p (->R 5))))
                      (throw (js/Error. "old-generation-failure"))))
      (defrecord R [x] P (p [_] (+ 10 x)))
      (def current (->R 7))
      (defmacro probe []
        (assert (= [3 17 4] nested))
        (let [a (try broken (catch :default e e))
              b (try broken (catch :default e e))]
          (assert (identical? a b))
          (assert (= "old-generation-failure" (subs (.-message a)
                    (- (count (.-message a)) 22)))))
        (assert (= 1 @hits))
        (assert (= [3 17 12 16] [(p old) (p current) (p (live)) (p (->R 6))]))
        42)
      (probe)
      (defonce broken 0)
      (probe)
      (def broken (->R 8))
      (defmacro repaired [] (assert (= 18 (p broken))) (assert (= 1 @hits)) 42)
      (repaired)"#;
    // The deliberately throwing application initializer is never executed.
    // These are actual macro assertions of sticky failure and current recovery.
    let js = light_cljs::compile_with_macros(source, &mut host).unwrap();
    assert!(js.contains("return 42"));
    assert_eq!(
        result(
            &light_cljs::compile_with_macros(
                "(defrecord R [x]) (defmacro clean [] (:x (->R 42))) (clean)",
                &mut host,
            )
            .unwrap()
        ),
        "42\n"
    );
}
