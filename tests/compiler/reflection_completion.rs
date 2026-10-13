//! Compiler-owned namespace reflection, source identity and phase separation.
use light_cljs::{
    MacroHost, analyze_with_macros,
    emitter::emit_js_unit,
    evaluator::{Evaluator, EvaluatorOptions},
    parse,
};
use std::{
    io::Write,
    process::{Command, Stdio},
};
fn both(source: &str, expected: &str) {
    let mut host = MacroHost::new();
    let program = analyze_with_macros(source, &parse(source).unwrap(), &mut host)
        .unwrap_or_else(|e| panic!("{source}\n{e}"));
    let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
    let value = evaluator
        .evaluate(&program)
        .unwrap_or_else(|e| panic!("{source}\n{e}"));
    assert_eq!(evaluator.print(value).unwrap(), expected);
    let script = format!(
        "const rt = {}; console.log(rt.core.prStr(({})(rt)));",
        light_cljs::evaluator::RUNTIME_SOURCE,
        emit_js_unit(&program)
    );
    // ns-publics may request metadata for every mapped core Var. Feed the
    // resulting program over stdin instead of relying on the OS argv limit.
    let mut child = Command::new("node")
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(script.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8(out.stdout).unwrap().trim(), expected);
}
#[test]
fn complete_upstream_var_test_runs_metadata_callbacks_in_both_engines() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/compiler-namespaces/definitions");
    let js = light_cljs::compile_namespace("cljs.var-test", &[root]).unwrap();
    // This existing source adapter retains both complete upstream definitions,
    // the :test callback assertions, and both function/Var test invocations.
    let script = format!("(() => {{ const run = {js}; return run(null); }})()");
    let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
    let value = evaluator.evaluate_js(&script).unwrap();
    assert_eq!(evaluator.print(value).unwrap(), "true");
    let out = Command::new("node")
        .args(["-e", &format!("console.log({script});")])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "true\n");
}
#[test]
fn core_public_vars_are_callable_with_source_metadata_in_both_phases() {
    both(
        r#"(defmacro from-publics []
      (let [match-var (get (ns-publics 'cljs.core) 're-matches)]
        (assert (var? match-var))
        (assert (= 're-matches (:name (meta match-var))))
        (assert (= "cljs.core" (ns-name (:ns (meta match-var)))))
        (assert (= "a" (match-var #"a" "a")))
        (assert (= "a" ((identity @match-var) #"a" "a")))
        (list 'quote :macro)))
      (let [v (get (ns-publics 'cljs.core) 're-matches)
            hooks (ns-interns 'cljs.core)]
        [(from-publics) (v #"a" "a") (= v #'re-matches)
         (string? (:doc (meta v))) (= "cljs.core" (ns-name (:ns (meta v))))
         (and (object? (#'js-obj)) (every? #(contains? hooks %) '[create-ns find-ns ns-name Namespace add-tap remove-tap tap> system-time flush]))])"#,
        "[:macro \"a\" true true true true]",
    );
}
#[test]
fn source_reflection_has_private_macro_metadata_and_lazy_identity() {
    both(
        r#"(def ^{:doc "ordinary"} value 41)
      (def ^:private hidden 9)
      (defmacro ^{:doc "expansion"} m [] 42)
      (defmacro inspect-view []
        (let [vars (ns-interns 'cljs.user) macro-var (get vars 'm)]
          (assert (= #'value (get vars 'value)))
          (assert (:macro (meta macro-var)))
          (assert (= "expansion" (:doc (meta macro-var))))
          42))
      [(inspect-view) (contains? (ns-publics 'cljs.user) 'hidden)
       (contains? (ns-interns 'cljs.user) 'hidden)
       (and (:macro (meta #'m)) (= #'m (resolve 'm)))
       (try @(get (ns-publics 'cljs.user) 'm) (catch :default e (boolean (re-find #"compile-time macro" (.-message e)))))
       (= 42 (inc @(get (ns-publics 'cljs.user) 'value))) ]"#,
        "[42 false true true true true]",
    );
    let mut host = MacroHost::new();
    light_cljs::compile_with_macros(
        r#"(def pending (js/connect))
      (defmacro inspect-only []
        (assert (= #'pending (get (ns-interns 'cljs.user) 'pending)))
        (assert (= 'pending (:name (meta (get (ns-publics 'cljs.user) 'pending))))) 42)
      (inspect-only)"#,
        &mut host,
    )
    .unwrap();
    let error = light_cljs::compile_with_macros(
        r#"(def pending (js/connect))
      (defmacro demand [] @(get (ns-publics 'cljs.user) 'pending)) (demand)"#,
        &mut host,
    )
    .unwrap_err();
    assert!(error.to_string().contains("connect"), "{error}");
}
#[test]
fn namespace_objects_keep_identity_equality_hash_and_explicit_host_objects() {
    both(
        r#"(def value 42)
      (let [a (create-ns 'created) b (create-ns 'created)
            host (create-ns 'host.space #js {:x 9})
            separate (Namespace. #js {} 'created)]
        [(nil? (find-ns nil)) (nil? (find-ns 'absent))
         (identical? a b) (= a separate) (= (hash a) (hash separate))
         (= 'created (ns-name a)) (= a (the-ns 'created))
         (= 'cljs.user (ns-name (find-ns 'cljs.user)))
         (contains? (set (map ns-name (all-ns))) 'created)
         (= 9 @(.findInternedVar host 'x))
         (nil? (.findInternedVar host 'absent))])"#,
        "[true true true true true true true true true true true]",
    );
}
#[test]
fn namespace_view_contains_core_binding_kinds_without_engine_roots() {
    use light_cljs::compiler_data::{Name, Source};
    use light_cljs::compiler_reader::{BindingKind, NamespaceView};
    let view = NamespaceView::new("main", &Source::new("main.cljs", ""));
    assert_eq!(
        view.binding(&Name::new(None, "re-matches")).unwrap().kind,
        BindingKind::Value
    );
    assert_eq!(
        view.binding(&Name::new(None, "when")).unwrap().kind,
        BindingKind::Macro
    );
    assert_eq!(
        view.binding(&Name::new(None, "IFn")).unwrap().kind,
        BindingKind::Protocol
    );
    assert_eq!(
        view.binding(&Name::new(None, "Namespace")).unwrap().kind,
        BindingKind::Type
    );
    assert_eq!(view.declaration_frontier, 0);
}
#[test]
fn namespace_refers_use_actual_public_var_identity() {
    use light_cljs::{
        EvalOptions, EvalSession,
        source_provider::{LoadRequest, SourceInput},
    };
    use std::rc::Rc;
    let mut session = EvalSession::new(EvalOptions {
        source_provider: Some(Rc::new(|request: &LoadRequest| {
            Ok(
                (String::from_utf16_lossy(request.namespace.name()) == "library").then(|| {
                    SourceInput::new("library.cljs", "(ns library) (defn add [x] (+ 41 x))")
                }),
            )
        })),
        ..Default::default()
    })
    .unwrap();
    let value = session.eval_string("(ns app (:require [library :refer [add]])) (let [v (get (ns-refers 'app) 'add)] [(= v #'library/add) (v 1)])").unwrap().value;
    assert_eq!(session.print(value).unwrap(), "[true 42]");
}
#[test]
fn namespace_api_arities_execute_in_applications_and_macros() {
    both(
        r#"(def ^:private private-value 7)
      (defn visible [] 42)
      (defn check-reflection []
        (let [a (create-ns 'fresh) b (create-ns 'explicit #js {:value 9})
              c (Namespace. #js {:other 1} 'fresh)
              publics (ns-publics 'cljs.user) interns (ns-interns 'cljs.user)
              imports (ns-imports 'cljs.user)]
          (assert (instance? Namespace a))
          (assert (= a c))
          (assert (= 'explicit (ns-name b)))
          (assert (identical? a (find-ns 'fresh)))
          (assert (= 9 @(.findInternedVar b 'value)))
          (assert (nil? (find-ns 'unknown)))
          (assert (= #'visible (get publics 'visible)))
          (assert (= 42 ((get publics 'visible))))
          (assert (not (contains? publics 'private-value)))
          (assert (= 7 @(get interns 'private-value)))
          (assert (= {} imports)) true))
      (defmacro check-in-macro [] (assert (check-reflection)) true)
      [(check-reflection) (check-in-macro)]"#,
        "[true true]",
    );
}
