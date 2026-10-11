//! Compiler macro integration, including transitional native adapter tests.
//! CLJS environment/resolution observations
//! are anchored in tests/reference/compiler-macros.md; tests need only Rust/Node.
use light_cljs::interpreter::{
    Arity, Name, NativeDefinition, NativeEffects, NativeMetadata, Reload, ResourceLimits,
};
use light_cljs::{
    MacroHost, MacroImport, analyze_with_macros, compile, compile_namespace,
    compile_namespace_with_macros, compile_with_macros, emit_js, parse,
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
fn host(source: &str, refers: &[&str]) -> MacroHost {
    let mut host = MacroHost::new();
    host.context_mut().eval_string(source).unwrap();
    host.set_imports(
        "cljs.user",
        vec![MacroImport {
            namespace: "macros".into(),
            alias: Some("m".into()),
            refers: refers.iter().map(|s| (*s).into()).collect(),
        }],
    )
    .unwrap();
    host
}

#[test]
fn two_runtime_namespaces_use_native_clj_macros_through_api_and_cli() {
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
fn locals_shadow_macros_but_explicit_macro_imports_survive_runtime_definitions() {
    let mut h = host("(ns macros) (defmacro value [] 7)", &["value"]);
    let source = r#"(def value 3)
        (js/console.log value (value) (m/value)
          (let [value (fn [] 9)] (value))
          (let [value (fn [] 9)] (macros/value)))"#;
    assert_eq!(
        execute(&compile_with_macros(source, &mut h).unwrap()),
        "3 7 7 9 7\n"
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
fn macro_aliases_do_not_create_runtime_aliases_or_expose_interpreter_vars() {
    let s = Sources::new();
    s.write(
        "api.clj",
        "(ns api) (def runtime-only 40) (defmacro value [] 7)",
    );
    s.write("runtime.cljs", "(ns runtime) (def value 5)");
    s.write("main.cljs", "(ns main (:require [runtime :as a]) (:require-macros [api :as a])) (js/console.log a/value (a/value))");
    assert_eq!(execute(&s.compile("main").unwrap()), "5 7\n");
    s.write(
        "main.cljs",
        "(ns main (:require-macros [api :as a])) a/runtime-only",
    );
    assert!(
        s.compile("main")
            .unwrap_err()
            .message
            .contains("unknown namespace or alias a")
    );
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
    assert!(compile_with_macros("macros/hidden", &mut h).is_err());
    assert!(compile_with_macros("m/read-hidden", &mut h).is_err());
}

#[test]
fn cljs_env_has_binding_records_context_namespace_and_unevaluated_form() {
    let mut h = host(
        r#"(ns macros)
      (defmacro inspect-env []
        (str (:context &env) "/" (:name (:ns &env)) "/" (ns-name *ns*) "/"
          (:op (get (:locals &env) 'x)) "/" (:local (get (:locals &env) 'x)) "/"
          (:arg-id (get (:locals &env) 'x)) "/" (contains? (:locals &env) 'self)))
      (defmacro syntax [x] (str (first &form) "/" x "/" (:line (meta &form))))
    "#,
        &["inspect-env", "syntax"],
    );
    assert_eq!(
        result(&compile_with_macros("(let [x (inspect-env)] x)", &mut h).unwrap()),
        ":expr/cljs.user/cljs.user////false\n"
    );
    assert_eq!(
        result(&compile_with_macros("(let [x 1] (inspect-env))", &mut h).unwrap()),
        ":return/cljs.user/cljs.user/:binding/:let//false\n"
    );
    assert_eq!(
        result(&compile_with_macros("((fn self [x] (inspect-env)) 1)", &mut h).unwrap()),
        ":return/cljs.user/cljs.user/:binding/:arg/0/true\n"
    );
    assert_eq!(
        result(&compile_with_macros("(syntax (undefined never-evaluated))", &mut h).unwrap()),
        "syntax/(undefined never-evaluated)/1\n"
    );
    let discarded = "(js/console.log (let [x 1] (js/console.log (inspect-env)) (inspect-env)))";
    assert_eq!(
        execute(&compile_with_macros(discarded, &mut h).unwrap()),
        ":expr/cljs.user/cljs.user/:binding/:let//false\n:return/cljs.user/cljs.user/:binding/:let//false\n"
    );
}

#[test]
fn macro_namespaces_load_once_with_clj_features_and_explicit_reload_is_observed() {
    let s = Sources::new();
    s.write(
        "macros.cljc",
        "(ns macros) #?(:clj (defmacro value [] 1) :cljs (defmacro value [] 9))",
    );
    s.write(
        "main.cljs",
        "(ns main (:require-macros [macros :refer [value]])) (js/console.log (value))",
    );
    let mut h = MacroHost::new();
    assert_eq!(execute(&s.compile_host("main", &mut h).unwrap()), "1\n");
    s.write("macros.cljc", "(ns macros) (defmacro value [] 2)");
    assert_eq!(execute(&s.compile_host("main", &mut h).unwrap()), "1\n");
    h.load_namespace("macros", Reload::Namespace).unwrap();
    assert_eq!(execute(&s.compile_host("main", &mut h).unwrap()), "2\n");
    assert_eq!(execute(&s.compile("main").unwrap()), "2\n");
    let mut isolated = MacroHost::new();
    assert!(compile_with_macros("(macros/value)", &mut isolated).is_err());
}

#[test]
fn macro_code_uses_clj_dependencies_and_caller_aliases_without_allowing_cljs_conditionals() {
    let s = Sources::new();
    s.write("helper.clj", "(ns helper) (defn select [x] x)");
    s.write(
        "macros.clj",
        "(ns macros (:require [helper :as h])) (defmacro identity-form [x] (h/select x))",
    );
    s.write("main.cljs", "(ns main (:require-macros [macros :refer [identity-form]])) (js/console.log (identity-form #?(:cljs 4 :clj 8)))");
    assert!(
        s.compile("main")
            .unwrap_err()
            .message
            .contains("ReaderConditional")
    );
    s.write(
        "macros.clj",
        "(ns macros (:require [helper :as h])) (defmacro keyword-name [x] (h/select (str x)))",
    );
    s.write("runtime.cljs", "(ns runtime)");
    s.write("main.cljs", "(ns main (:require [runtime :as r]) (:require-macros [macros :as m :refer [keyword-name]])) (js/console.log (keyword-name ::m/a) (keyword-name ::r/b) (keyword-name ::c))");
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
        "macros.clj",
        "(ns macros) (def value 1) (defmacro hidden {:private true} [] 4) (defmacro visible [] 5)",
    );
    for (header, message) in [
        ("(:require-macros [macros :refer [value]])", "public macro"),
        ("(:require-macros [macros :refer [hidden]])", "public macro"),
        (
            "(:require-macros [macros :refer [visible visible]])",
            "duplicate macro refer",
        ),
        ("(:require-macros [missing :as m])", "MissingNamespace"),
        (
            "(:require-macros [macros :refer :all])",
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
        "(ns main (:require-macros [macros :as m])) (m/visible)",
    );
    s.write("macros.cljc", "(ns macros) (defmacro visible [] 5)");
    assert_eq!(result(&s.compile("main").unwrap()), "5\n");
}

#[test]
fn native_host_macros_use_the_same_form_env_and_arity_abi() {
    static MACRO: NativeDefinition = NativeDefinition {
        metadata: NativeMetadata {
            namespace: "host",
            name: "answer",
            arity: Arity::Exact(3),
            effects: NativeEffects::Pure,
            errors: &[],
        },
        callback: |c, args| {
            let env = args[1].get(&c.keyword(Name::new(None, "locals")))?.unwrap();
            let binding = env.get(&c.symbol(Name::new(None, "x")))?.unwrap();
            assert!(binding.get(&c.keyword(Name::new(None, "name")))?.is_some());
            Ok(args[2].clone())
        },
    };
    let mut h = MacroHost::new();
    h.register_macro(&MACRO);
    h.set_imports(
        "cljs.user",
        vec![MacroImport {
            namespace: "host".into(),
            alias: Some("h".into()),
            refers: vec![],
        }],
    )
    .unwrap();
    let source = "(let* [x 17] (h/answer x))";
    let syntax = parse(source).unwrap();
    let original = syntax.clone();
    let program = analyze_with_macros(source, &syntax, &mut h).unwrap();
    assert_eq!(
        syntax, original,
        "analysis must not mutate caller-owned syntax"
    );
    assert_eq!(result(&emit_js(&program)), "17\n");
    let sources = Sources::new();
    sources.write(
        "main.cljs",
        "(ns main (:require-macros [host :as h])) (let [x 19] (h/answer x))",
    );
    assert_eq!(
        result(&sources.compile_host("main", &mut h).unwrap()),
        "19\n"
    );
    assert!(
        compile_with_macros("(h/answer)", &mut h)
            .unwrap_err()
            .to_string()
            .contains("Arity")
    );
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
    h.context_mut().set_resource_limits(ResourceLimits {
        fuel: Some(500),
        ..ResourceLimits::default()
    });
    assert!(
        compile_with_macros("(spin)", &mut h)
            .unwrap_err()
            .to_string()
            .contains("Fuel")
    );
    // Per-compilation counters are fresh after failure; no poisoned expansion cache.
    assert_eq!(
        result(&compile_with_macros("(one)", &mut h).unwrap()),
        "1\n"
    );
}

#[test]
fn macro_call_namespace_binding_is_restored_and_do_context_matches_cljs() {
    let mut h = host(
        "(ns macros) (defmacro context [] (str (:context &env)))",
        &["context"],
    );
    let before = h.context_mut().current_namespace();
    assert_eq!(
        execute(
            &compile_with_macros("(js/console.log (do 1 (context)) (do (context)))", &mut h)
                .unwrap()
        ),
        ":return :expr\n"
    );
    assert!(before.same_identity(&h.context_mut().current_namespace()));
    assert!(compile_with_macros("(context 1)", &mut h).is_err());
    assert!(before.same_identity(&h.context_mut().current_namespace()));
}

#[test]
fn runtime_caller_namespace_does_not_count_as_loaded_macro_code() {
    let s = Sources::new();
    s.write("same.cljs", "(ns same) (let [x 1] x)");
    s.write("same.clj", "(ns same) (defmacro value [] 42)");
    s.write(
        "main.cljs",
        "(ns main (:require-macros [same :refer [value]])) (value)",
    );
    let mut h = MacroHost::new();
    s.compile_host("same", &mut h).unwrap();
    assert_eq!(result(&s.compile_host("main", &mut h).unwrap()), "42\n");
}

#[test]
fn macro_namespace_effects_are_shared_once_per_host_not_across_default_compilations() {
    let s = Sources::new();
    s.write(
        "macros.clj",
        "(ns macros) (def counter (atom 0)) (defmacro next-value [] (swap! counter inc))",
    );
    s.write(
        "dep.cljs",
        "(ns dep (:require-macros [macros :refer [next-value]])) (def value (next-value))",
    );
    s.write("main.cljs", "(ns main (:require [dep :as d]) (:require-macros [macros :refer [next-value]])) (js/console.log d/value (next-value))");
    assert_eq!(execute(&s.compile("main").unwrap()), "1 2\n");
    assert_eq!(execute(&s.compile("main").unwrap()), "1 2\n");
    let mut h = MacroHost::new();
    assert_eq!(execute(&s.compile_host("main", &mut h).unwrap()), "1 2\n");
    assert_eq!(execute(&s.compile_host("main", &mut h).unwrap()), "3 4\n");
}

#[test]
fn macro_statistics_attribute_native_and_interpreted_expansion_work() {
    let mut h = host("(ns macros) (defmacro value [] '(let [x 3] x))", &["value"]);
    h.context_mut().set_macro_diagnostics(true);
    compile_with_macros("(value)", &mut h).unwrap();
    let stats = h.context_mut().macro_expansion_stats();
    let custom = stats
        .iter()
        .find(|s| s.name == Name::new(Some("macros"), "value"))
        .unwrap();
    assert_eq!(custom.expansions, 1);
    assert!(custom.generated_forms > 1);
    assert!(custom.definition.is_some());
    assert!(
        stats
            .iter()
            .any(|s| s.name == Name::new(Some("clojure.core"), "let"))
    );
}

#[test]
fn macro_import_effects_follow_runtime_dependency_analysis_order() {
    let s = Sources::new();
    s.write(
        "shared.clj",
        "(ns shared) (def state (atom 0)) (defmacro value [] @state)",
    );
    s.write(
        "b.cljs",
        "(ns b (:require-macros [shared :refer [value]])) (def x (value))",
    );
    s.write(
        "modify.clj",
        "(ns modify (:require [shared :as s])) (reset! s/state 9)",
    );
    s.write(
        "a.cljs",
        "(ns a (:require [b :as b]) (:require-macros modify)) (js/console.log b/x)",
    );
    assert_eq!(execute(&s.compile("a").unwrap()), "0\n");
}

#[test]
fn default_interpreter_user_identity_does_not_hide_user_macro_source() {
    let s = Sources::new();
    s.write("user.clj", "(ns user) (defmacro value [] 42)");
    s.write(
        "main.cljs",
        "(ns main (:require-macros [user :refer [value]])) (value)",
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
    assert_eq!(
        h.context_mut()
            .eval_string("@macros/counter")
            .unwrap()
            .inspect(),
        light_cljs::interpreter::ValueView::Integer(0)
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

    static SURROGATE: NativeDefinition = NativeDefinition {
        metadata: NativeMetadata {
            namespace: "host",
            name: "surrogate",
            arity: Arity::Exact(2),
            effects: NativeEffects::Pure,
            errors: &[],
        },
        callback: |c, _| Ok(c.keyword(Name::from_utf16(Some(&[0xd800]), &[0xd800]))),
    };
    h.register_macro(&SURROGATE);
    h.set_imports(
        "cljs.user",
        vec![MacroImport {
            namespace: "host".into(),
            alias: Some("h".into()),
            refers: vec![],
        }],
    )
    .unwrap();
    let source = "(let [k (h/surrogate)] (js/console.log (.charCodeAt (name k) 0) (.charCodeAt (namespace k) 0)))";
    assert_eq!(
        execute(&compile_with_macros(source, &mut h).unwrap()),
        "55296 55296\n"
    );
}

#[test]
fn direct_auto_keywords_use_the_same_macro_alias_precedence_as_macro_input() {
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
    sources.write("macros.clj", "(ns macros) (defmacro identity-form [x] x)");
    sources.write("main.cljs", "(ns main (:require [runtime :as m]) (:require-macros [macros :as m])) (js/console.log (namespace ::m/x) (namespace (m/identity-form ::m/x)))");
    assert_eq!(
        execute(&sources.compile("main").unwrap()),
        "macros macros\n"
    );
}

#[test]
fn quoted_macro_values_preserve_symbols_effects_and_generated_provenance() {
    let mut h = host(
        "(ns macros) (def counter (atom 0)) (defmacro danger [] (swap! counter inc) 99) (defmacro data [] '(quote [unresolved-name (danger) a/x])) (defmacro invalid [] '(quote [ok \\a]))",
        &["danger", "data", "invalid"],
    );
    let source = "(js/console.log (name (nth (data) 0)) (name (first (nth (data) 1))) (= (nth (data) 2) 'a/x) (= '(danger) (list 'danger)))";
    assert_eq!(
        execute(&compile_with_macros(source, &mut h).unwrap()),
        "unresolved-name danger true true\n"
    );
    assert_eq!(
        h.context_mut()
            .eval_string("@macros/counter")
            .unwrap()
            .inspect(),
        light_cljs::interpreter::ValueView::Integer(0)
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
    static QUOTED: NativeDefinition = NativeDefinition {
        metadata: NativeMetadata {
            namespace: "host",
            name: "quoted",
            arity: Arity::Exact(2),
            effects: NativeEffects::Pure,
            errors: &[],
        },
        callback: |c, _| {
            let quote = c.symbol(Name::new(None, "quote"));
            let value = c.symbol(Name::from_utf16(Some(&[0xd800]), &[0xdc00]));
            Ok(c.list(&[quote, value])?)
        },
    };
    static EXECUTABLE: NativeDefinition = NativeDefinition {
        metadata: NativeMetadata {
            namespace: "host",
            name: "executable",
            arity: Arity::Exact(2),
            effects: NativeEffects::Pure,
            errors: &[],
        },
        callback: |c, _| Ok(c.symbol(Name::from_utf16(Some(&[0xd800]), &[0xdc00]))),
    };
    let mut h = host("(ns macros)", &[]);
    h.register_macro(&QUOTED);
    h.register_macro(&EXECUTABLE);
    h.set_imports(
        "cljs.user",
        vec![MacroImport {
            namespace: "host".into(),
            alias: Some("h".into()),
            refers: vec![],
        }],
    )
    .unwrap();
    let source = "(let [s (h/quoted)] (js/console.log (.charCodeAt (name s) 0) (.charCodeAt (namespace s) 0)))";
    assert_eq!(
        execute(&compile_with_macros(source, &mut h).unwrap()),
        "56320 55296\n"
    );
    let error = compile_with_macros("(h/executable)", &mut h).unwrap_err();
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
    let expected = (0..32).map(|i| format!("{i}\n")).collect::<String>() + "16 true true\n";
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
    assert_eq!(&source[error.span], "{:a missing}");
}

#[test]
fn generated_map_keys_keep_reader_name_parts_before_runtime_spelling_equality() {
    static MAP_NAMES: NativeDefinition = NativeDefinition {
        metadata: NativeMetadata {
            namespace: "host",
            name: "map-names",
            arity: Arity::Exact(2),
            effects: NativeEffects::Pure,
            errors: &[],
        },
        callback: |c, _| {
            let first = c.keyword(Name::new(None, "a/b"));
            let second = c.keyword(Name::new(Some("a"), "b"));
            Ok(c.map(&[(first, c.integer(1)), (second, c.integer(2))])?)
        },
    };
    let mut h = host("(ns macros)", &[]);
    h.register_macro(&MAP_NAMES);
    h.set_imports(
        "cljs.user",
        vec![MacroImport {
            namespace: "host".into(),
            alias: Some("h".into()),
            refers: vec![],
        }],
    )
    .unwrap();
    assert_eq!(
        result(&compile_with_macros("(count (h/map-names))", &mut h).unwrap()),
        "1\n"
    );
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
    let expected = (0..16).map(|i| format!("{i}\n")).collect::<String>() + "16 true true\n";
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
    assert_eq!(&source[error.span], "#{:a missing}");
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
            "bad/macros.clj",
            &format!("(ns bad.macros) (defmacro emit-bad [] {body})"),
        );
        let source = "(ns bad.main (:require-macros [bad.macros :as m]))\n; λ\n(m/emit-bad)";
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
        "batch/macros.clj",
        include_str!("../examples/collections/batch/macros.clj"),
    );
    sources.write(
        "user/data.cljs",
        "(ns user.data) (def assoc (fn* [x] x)) (def vector (fn* [x] x))",
    );
    sources.write(
        "main.cljs",
        r#"(ns main (:require [cljs.core :as c] [user.data :as m])
      (:require-macros [batch.macros :as m]))
      (def assoc (fn* [x] x))
      (def saved c/get)
      (def get (fn* [x] x))
      (let [conj (fn* [x] x) reduce (fn* [x] x) + (fn* [x] x)
            changed (m/append-item {:items [1 2]} :items 3)]
        (js/console.log (c/reduce c/+ 0 (saved changed :items))
          (assoc 7) (get 8) (m/assoc 9) (m/vector 10) (conj 11) (reduce 12) (+ 13)
          (namespace ::m/x)))"#,
    );
    assert_eq!(
        execute(&sources.compile("main").unwrap()),
        "6 7 8 9 10 11 12 13 batch.macros\n"
    );
}

#[test]
fn collection_macro_compilation_obeys_cancellation_and_recovers_with_the_same_host() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/collections");
    let mut host = MacroHost::new();
    let token = light_cljs::interpreter::CancellationToken::default();
    token.cancel();
    host.context_mut().set_cancellation_token(Some(token));
    let error = compile_namespace_with_macros("batch.main", std::slice::from_ref(&root), &mut host)
        .unwrap_err();
    assert!(error.to_string().contains("Cancelled"), "{error}");
    host.context_mut().set_cancellation_token(None);
    let js = compile_namespace_with_macros("batch.main", &[root], &mut host).unwrap();
    assert_eq!(
        execute(&format!("const run = {js}; console.log(typeof run);")),
        "data\nops\nmain\nfunction\n"
    );
}

#[test]
fn self_required_macro_companions_are_inferred_with_runtime_refers_and_renames() {
    let s = Sources::new();
    s.write(
        "shared.cljc",
        r#"(ns shared #?(:cljs (:require-macros [shared])))
       #?(:clj (defmacro add [x] `(cljs.core/+ ~x 3)) :cljs (def value 8))"#,
    );
    s.write(
        "main.cljs",
        r#"(ns main (:require [shared :as s :refer [value add] :rename {add plus}]))
        (js/console.log value (plus 4) (s/add 6) (= ::s/k :shared/k))"#,
    );
    assert_eq!(execute(&s.compile("main").unwrap()), "8 7 9 true\n");
}

#[test]
fn explicit_macro_sugar_clj_precedence_and_macro_renames_execute_in_javascript() {
    let s = Sources::new();
    s.write("lib.cljs", "(ns lib) (def value 9)");
    s.write("lib.cljc", "(ns lib) #?(:clj (defmacro add [x] 999))");
    s.write(
        "lib.clj",
        "(ns lib) (defmacro add [x] `(cljs.core/+ ~x 2)) (defmacro kw [x] (str x))",
    );
    for options in [
        ":include-macros true",
        ":include-macros false",
        ":refer-macros [add]",
    ] {
        s.write("main.cljs", &format!("(ns main (:require [lib :as l {options}])) (js/console.log l/value (l/add 5) (l/kw ::l/a))"));
        assert_eq!(execute(&s.compile("main").unwrap()), "9 7 :lib/a\n");
    }
    s.write(
        "main.cljs",
        "(ns main (:require [lib :refer-macros [add]])) (js/console.log (add 4))",
    );
    assert_eq!(execute(&s.compile("main").unwrap()), "6\n");
    s.write("main.cljs", "(ns main (:require-macros [lib :refer [add] :rename {add sum}])) (js/console.log (sum 4) (= `sum 'lib/add))");
    assert_eq!(execute(&s.compile("main").unwrap()), "6 true\n");
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
    s.write(
        "main.cljs",
        "(ns main (:require [lib :as l :include-macros true]))\n(l/bad)",
    );
    let error = s.compile("main").unwrap_err();
    assert_eq!(error.line_column, Some((2, 1)));
    assert!(error.message.contains("recur"));
}

#[test]
fn shared_conditional_arguments_are_selected_before_native_macro_lowering() {
    let s = Sources::new();
    s.write("lib.clj", "(ns lib) (defmacro id [x] x)");
    s.write("main.cljc", "(ns main (:require-macros [lib :refer [id]])) (js/console.log (id #?(:clj jvm-symbol :cljs 7)))");
    assert_eq!(execute(&s.compile("main").unwrap()), "7\n");
}

#[test]
fn namespace_metadata_and_docstrings_reach_the_macro_environment_and_refresh_on_reuse() {
    let s = Sources::new();
    s.write("macros.clj", "(ns macros) (defmacro read-meta [] (str (:foo (meta (:name (:ns &env)))) \"/\" (:doc (:ns &env))))");
    s.write("main.cljs", "(ns ^{:foo old} main \"doc\" {:foo new} (:require-macros [macros :refer [read-meta]])) (js/console.log (read-meta))");
    let mut h = MacroHost::new();
    assert_eq!(
        execute(&s.compile_host("main", &mut h).unwrap()),
        "new/doc\n"
    );
    s.write(
        "main.cljs",
        "(ns main (:require-macros [macros :refer [read-meta]])) (js/console.log (read-meta))",
    );
    assert_eq!(execute(&s.compile_host("main", &mut h).unwrap()), "/\n");
}

#[test]
fn explicit_use_and_use_macros_preserve_renamed_namespace_identity() {
    let s = Sources::new();
    s.write("lib.cljs", "(ns lib) (def f (fn* [] 123))");
    s.write("main.cljs", "(ns main (:use [lib :only [f] :rename {f renamed}]) (:use-macros [clojure.core :only [when] :rename {when always}])) (js/console.log (renamed) (= renamed lib/f) (always true 42))");
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
    sources.write("controls.clj", "(ns controls) (def n (atom 0)) (defmacro marked [] (swap! n inc)) (defmacro generated [] '(let [out #js []] (while (< (.-length out) 3) (.push out (.-length out))) (dotimes [i 2.5] (.push out i)) out)) (defmacro native-count [] (let [a (atom 0)] (dotimes [i 2.5] (swap! a inc)) @a)) (defmacro enabled [] *assert*)");
    sources.write("main.cljs", "(ns main (:require [cljs.core :as c]) (:require-macros [controls :as m] [cljs.core :refer [assert] :rename {assert check}])) (js/console.log (.-length (m/generated)) (m/native-count) (m/enabled) (str (c/condp = 2 2 :ok :bad)) (case 1 (1 2 3) (m/marked) 0) (m/marked) (check true))");
    assert_eq!(
        execute(&sources.compile("main").unwrap()),
        "6 2 true :ok 1 2 null\n"
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
    sources.write("config.clj", "(ns config) (defmacro enabled [] *assert*) (defmacro guarded [] (when *assert* '(assert false))) (defmacro native-check [] (assert false) 3) (defmacro expanded [] (binding [*assert* false] (macroexpand '(assert false))))");
    sources.write(
        "main.cljs",
        "(ns main (:require-macros [config :as c])) (js/console.log (c/enabled) (c/guarded) (c/native-check) (c/expanded))",
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
          (with-meta [x] {:value (list '+ x 1) :line 99 :file "macro.clj"}))
        (defmacro quoted [] (list 'quote (with-meta 'sample {:marker true :line 99})))
        (defmacro ^{:purpose "compiler"} definition [] (:purpose (meta #'definition)))"#,
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
        host.disable_native_evaluation();
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
            host.disable_native_evaluation();
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
        host.disable_native_evaluation();
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
        host.disable_native_evaluation();
        let p = analyze_with_macros(&source, &parse(&source).unwrap(), &mut host).unwrap();
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
