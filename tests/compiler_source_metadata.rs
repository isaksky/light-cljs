//! Definition provenance is independent of user metadata and execution engine.
use light_cljs::{
    EvalOptions, EvalSession, MacroHost, analyze_with_macros,
    emitter::emit_js_unit,
    evaluator::{Evaluator, EvaluatorOptions, RUNTIME_SOURCE},
    parse,
    source_provider::SourceInput,
};
use std::process::Command;
fn both(source: &str, host: &mut MacroHost, expected: &str) {
    let p = analyze_with_macros(source, &parse(source).unwrap(), host).unwrap();
    let e = Evaluator::new(EvaluatorOptions::default()).unwrap();
    assert_eq!(e.print(e.evaluate(&p).unwrap()).unwrap(), expected);
    let js = format!(
        "const rt = {RUNTIME_SOURCE}; console.log(rt.core.prStr(({})(rt)));",
        emit_js_unit(&p)
    );
    let out = Command::new("node").args(["-e", &js]).output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8(out.stdout).unwrap().trim(), expected);
}
#[test]
fn reserved_metadata_and_macro_definitions_keep_source_sites_in_both_engines() {
    let source = r#"(def ^{:name :fake :ns :fake :file "fake" :line 0 :column 0 :doc "old"} x "new" 1)
(defonce ^{:file "fake" :line 0 :column 0} y 2)
(declare ^{:file "fake" :line 0 :column 0} z)
(defmacro make-def [] (list 'def (with-meta 'generated {:file "fake" :line 0 :column 0}) 3))
  (make-def)
(defmacro inspect-source []
  (assert (= "cljs.user" (:file (meta #'x))))
  (assert (= "cljs.user" (:file (meta #'generated))))
  (list 'quote [(:line (meta #'generated)) (:column (meta #'generated))]))
[(= 'x (:name (meta #'x))) (= "cljs.user" (ns-name (:ns (meta #'x))))
 (:file (meta #'x)) (:line (meta #'x)) (:column (meta #'x)) (:doc (meta #'x))
 [(:file (meta #'y)) (:line (meta #'y)) (:column (meta #'y))]
 [(:file (meta #'z)) (:line (meta #'z)) (:column (meta #'z)) (:declared (meta #'z))]
 [(:file (meta #'generated)) (:line (meta #'generated)) (:column (meta #'generated))]
 (inspect-source)]"#;
    let mut host = MacroHost::new();
    both(
        source,
        &mut host,
        "[true true \"cljs.user\" 1 1 \"new\" [\"cljs.user\" 2 1] [\"cljs.user\" 3 1 true] [\"cljs.user\" 5 3] [5 3]]",
    );
    assert!(light_cljs::compile_with_macros("(defmacro bad [] missing) (bad)", &mut host).is_err());
    both(
        "\n(def ^{:file \"fake\"} fresh 1) [(:file (meta #'fresh)) (:line (meta #'fresh))]",
        &mut host,
        "[\"cljs.user\" 2]",
    );
}
#[test]
fn incremental_files_errors_and_fresh_sessions_preserve_source_identity() {
    let mut session = EvalSession::new(EvalOptions::default()).unwrap();
    let source = SourceInput::new(
        "one\\\"λ.cljs",
        "\n  (def ^{:file \"fake\"} x 1)\n(defmacro make-def [] (list 'def 'generated 2))",
    );
    let v = session.eval_source(&source).unwrap().value;
    session.release(v).unwrap();
    let v = session.eval_source(&SourceInput::new("two.cljs", "\n    (make-def)\n[(:file (meta #'x)) (:line (meta #'x)) (:column (meta #'x)) (:file (meta #'generated)) (:line (meta #'generated)) (:column (meta #'generated))]")).unwrap().value;
    assert_eq!(
        session.print(v).unwrap(),
        "[\"one\\\\\\\"λ.cljs\" 2 3 \"two.cljs\" 2 5]"
    );
    session.release(v).unwrap();
    let v = session
        .eval_source(&SourceInput::new(
            "once-first.cljs",
            "(defonce ^{:file \"fake\"} once-value 10)",
        ))
        .unwrap()
        .value;
    session.release(v).unwrap();
    let v = session.eval_source(&SourceInput::new("once-second.cljs", "\n (defonce ^{:file \"fake\"} once-value 20)\n[once-value (:file (meta #'once-value)) (:line (meta #'once-value)) (:column (meta #'once-value))]")).unwrap().value;
    assert_eq!(session.print(v).unwrap(), "[10 \"once-second.cljs\" 2 2]");
    session.release(v).unwrap();
    assert!(
        session
            .eval_source(&SourceInput::new("bad.cljs", "(def failed missing)"))
            .is_err()
    );
    let v = session
        .eval_string(
            "(def ^{:file \"fake\"} after 3) [(:file (meta #'after)) (:line (meta #'after))]",
        )
        .unwrap()
        .value;
    assert_eq!(session.print(v).unwrap(), "[\"<eval>.cljs\" 1]");
    session.release(v).unwrap();
    let mut fresh = session.fresh_session().unwrap();
    assert!(fresh.eval_string("x").is_err());
    let v = fresh
        .eval_source(&SourceInput::new(
            "fresh.cljs",
            "(def x 0) (:file (meta #'x))",
        ))
        .unwrap()
        .value;
    assert_eq!(fresh.print(v).unwrap(), "\"fresh.cljs\"");
}
#[test]
fn ordinary_namespace_files_and_macro_generated_definitions_keep_paths() {
    let root =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/source-metadata");
    let js = light_cljs::compile_namespace("metadata.main", std::slice::from_ref(&root)).unwrap();
    let expected = "true";
    let e = Evaluator::new(EvaluatorOptions::default()).unwrap();
    assert_eq!(e.print(e.evaluate_js(&js).unwrap()).unwrap(), expected);
    let out = Command::new("node")
        .args(["-e", &format!("console.log({js});")])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8(out.stdout).unwrap().trim(), "true");
}

#[test]
fn generated_type_and_record_factories_keep_their_declaration_source() {
    both(
        r#"
  (deftype T [x])
    (defrecord R [x])
(defmacro inspect-factories []
  (assert (= "cljs.user" (:file (meta #'->R))))
  (assert (= [3 5] [(:line (meta #'map->R)) (:column (meta #'map->R))]))
  true)
[(inspect-factories)
 [(:file (meta #'->T)) (:line (meta #'->T)) (:column (meta #'->T))]
 [(:file (meta #'->R)) (:line (meta #'->R)) (:column (meta #'->R))]
 [(:file (meta #'map->R)) (:line (meta #'map->R)) (:column (meta #'map->R))]
 (:doc (meta #'->R)) (:arglists (meta #'->R))]"#,
        &mut MacroHost::new(),
        "[true [\"cljs.user\" 2 3] [\"cljs.user\" 3 5] [\"cljs.user\" 3 5] \"Positional factory function for cljs.user/R.\" ([x])]",
    );
}
