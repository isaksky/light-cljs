//! Native compiler macro integration. CLJS environment/resolution observations
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
    assert!(
        compile_with_macros("(too-big)", &mut h)
            .unwrap_err()
            .to_string()
            .contains("safe integers")
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
        "(ns macros) (defmacro bad [] '(loop* [x 1] x)) (defmacro bad-vector [] [1 2]) (defmacro explode [] (throw (ex-info \"broken\" {})))",
        &["bad", "bad-vector", "explode"],
    );
    for (call, message) in [
        ("(bad)", "generated by bad"),
        ("(bad-vector)", "generated by bad-vector"),
        ("(explode)", "broken"),
    ] {
        let source = format!("; λ\n{call}");
        let error = compile_with_macros(&source, &mut h).unwrap_err();
        assert!(error.to_string().contains(message), "{error}");
        let light_cljs::CompileError::Analysis(error) = error else {
            panic!()
        };
        assert_eq!(error.span, 5..source.len());
    }
    for source in [
        "(fn [x] {:pre [x]} x)",
        "(let [[x] 1] x)",
        "(fn ([x] x) ([] 1))",
        "(fn [& xs] xs)",
    ] {
        let error = compile(source).unwrap_err().to_string();
        assert!(error.contains("macro expansion"), "{source}: {error}");
    }
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
        (
            "(:require-macros [macros :as m] [macros :as m])",
            "duplicate macro alias",
        ),
        ("(:require-macros [missing :as m])", "MissingNamespace"),
        (
            "(:require-macros [macros :refer :all])",
            "vector of simple symbols",
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
    assert!(
        s.compile("main")
            .unwrap_err()
            .message
            .contains("AmbiguousSources")
    );
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
