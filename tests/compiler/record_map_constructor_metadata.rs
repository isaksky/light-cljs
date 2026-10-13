//! Unary map factory metadata with complete original source and actual macro demand.
use light_cljs::{MacroHost, compile_namespace_with_macros};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};
const ORIGINAL: &str = "(defrecord R [x y]) [(:arglists (meta (var ->R))) (mapv count (:arglists (meta (var map->R)))) (ns-name (:ns (meta (var ->R))))]";
const ROWS: &[(&str, &str, &str)] = &[
    (
        "sci-execution-sci-record-constructor-metadata",
        ORIGINAL,
        r#"{"id":"sci-execution-sci-record-constructor-metadata","outcome":{"status":"returned","value":{"$vector":[{"$list":[{"$vector":[{"$symbol":{"namespace":null,"name":"x","spelling":"x"}},{"$symbol":{"namespace":null,"name":"y","spelling":"y"}}]}]},{"$vector":[1]},"user"]}},"trace":[],"stdout":""}"#,
    ),
    (
        "record-map-constructor-controls",
        include_str!("../fixtures/record-map-constructor-metadata/controls.cljs"),
        r#"{"id":"record-map-constructor-controls","outcome":{"status":"returned","value":true},"trace":[],"stdout":""}"#,
    ),
];
fn host() -> MacroHost {
    let mut host = MacroHost::new();
    host.evaluator_options.timeout = std::time::Duration::from_secs(120);
    host
}
fn capture(source: &str) -> String {
    let anchor = " = // Original bounded runtime";
    if source.contains(anchor) {
        assert_eq!(source.matches(anchor).count(), 1);
        source.replacen(
            anchor,
            " = globalThis.lightRuntime = // Original bounded runtime",
            1,
        )
    } else {
        source.into()
    }
}
fn node(script: &str) -> String {
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
    assert!(out.stderr.is_empty(), "unexpected Node stderr");
    String::from_utf8(out.stdout).unwrap().trim_end().into()
}
#[test]
fn record_factory_metadata_executes_in_actual_phases_and_lifecycles() {
    let artifacts = std::env::var_os("LIGHT_RECORD_METADATA_ARTIFACTS")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!("light-record-metadata-{}", std::process::id()))
        });
    fs::create_dir_all(&artifacts).unwrap();
    let runtime = light_cljs::compile("[]").unwrap();
    fs::write(artifacts.join("observer-runtime.js"), &runtime).unwrap();
    let mut reused = host();
    let warmup = "(defmacro arity-ready [] (+ 20 22)) (arity-ready)";
    let warmup_js = light_cljs::compile_with_macros(warmup, &mut reused).unwrap();
    fs::write(artifacts.join("warmup.cljs"), warmup).unwrap();
    fs::write(artifacts.join("warmup.js"), &warmup_js).unwrap();
    for &(id, source, expected) in ROWS {
        for lifecycle in ["fresh", "reused"] {
            for phase in ["application", "macro"] {
                let macro_observer = format!(
                    "(({})(rt))({id:?}, ~{{}})",
                    include_str!("../support/sci_execution_observation.js")
                );
                let program = if phase == "macro" {
                    format!(
                        "(ns user)\n(defn arity-original [observe] (do {source}))\n(defmacro arity-witness [] (js* {macro_observer:?} arity-original))\n(defn run [] (arity-witness))\nrun\n"
                    )
                } else {
                    format!("(ns user)\n(defn run [observe] (do {source}))\nrun\n")
                };
                let root = artifacts.join(id).join(lifecycle).join(phase);
                fs::create_dir_all(&root).unwrap();
                fs::write(root.join("user.cljs"), &program).unwrap();
                let mut fresh = host();
                let actual_host = if lifecycle == "fresh" {
                    &mut fresh
                } else {
                    &mut reused
                };
                let js =
                    compile_namespace_with_macros("user", std::slice::from_ref(&root), actual_host)
                        .unwrap_or_else(|e| {
                            println!(
                                "record-metadata-compile-error\t{id}\t{lifecycle}\t{phase}\t{e}"
                            );
                            panic!("{id} {lifecycle} {phase}: {e}")
                        });
                fs::write(root.join("compiled.js"), &js).unwrap();
                let script = if phase == "macro" {
                    format!("(() => {{const run={};return run();}})()", capture(&js))
                } else {
                    format!(
                        "(() => {{void {};const run={};return ({})(globalThis.lightRuntime)('{id}',observe=>run(observe));}})()",
                        capture(&runtime),
                        capture(&js),
                        include_str!("../support/sci_execution_observation.js")
                    )
                };
                fs::write(root.join("script.js"), &script).unwrap();
                let runtime = rquickjs::Runtime::new().unwrap();
                let context = rquickjs::Context::full(&runtime).unwrap();
                let quickjs_input = format!("(0, eval)({script:?})");
                fs::write(root.join("quickjs.stdin"), &quickjs_input).unwrap();
                let quickjs = context.with(|ctx| {
                    ctx.eval::<String, _>(quickjs_input.as_bytes())
                        .unwrap_or_else(|e| {
                            panic!("{id} {lifecycle} {phase} observer: {e:?} {:?}", ctx.catch())
                        })
                });
                fs::write(root.join("quickjs.raw"), &quickjs).unwrap();
                assert_eq!(quickjs, expected, "{id} {lifecycle} {phase} QuickJS");
                println!(
                    "record-metadata-observation\t{id}\t{lifecycle}\t{phase}\tQuickJS\t{quickjs}"
                );
                let input = format!(
                    "const sha=x=>require('node:crypto').createHash('sha256').update(x).digest('hex');console.log(sha({js:?})+'\\t'+sha({program:?})+'\\t'+sha({script:?}));console.log({script});"
                );
                fs::write(root.join("node.stdin"), &input).unwrap();
                let packet = node(&input);
                fs::write(root.join("node.stdout"), format!("{packet}\n")).unwrap();
                let (hashes, actual) = packet
                    .split_once('\n')
                    .expect("actual Node input hash packet");
                println!("record-metadata-artifacts\t{id}\t{lifecycle}\t{phase}\t{hashes}");
                assert_eq!(actual, expected, "{id} {lifecycle} {phase} Node");
                println!("record-metadata-observation\t{id}\t{lifecycle}\t{phase}\tNode\t{actual}");
            }
        }
    }
}

#[test]
fn reflected_factory_metadata_does_not_force_unrelated_roots_and_failure_recovers() {
    for reused in [false, true] {
        let mut host = host();
        if reused {
            light_cljs::compile_with_macros("(defmacro ready [] 42) (ready)", &mut host).unwrap();
        }
        let source = r#"
(def roots (atom 0))
(def unrelated (swap! roots inc))
(defrecord Pending [x y])
(defmacro failed []
  (assert (= [1] (mapv count (:arglists (meta #'map->Pending)))))
  (assert (= [2] (mapv count (:arglists (meta #'->Pending)))))
  (assert (zero? @roots))
  (throw (ex-info "verified unary metadata before failure" {})))
(failed)
"#;
        let error = light_cljs::compile_with_macros(source, &mut host)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("verified unary metadata before failure"),
            "{error}"
        );
        let recovered = r#"
(defrecord Recovered [x y z])
(defmacro recovered []
  (and (= [1] (mapv count (:arglists (meta #'map->Recovered))))
       (= [3] (mapv count (:arglists (meta #'->Recovered))))
       (= [1 2 3] (mapv #(get (map->Recovered {:x 1 :y 2 :z 3}) %) [:x :y :z]))))
(pr-str (recovered))
"#;
        let js = light_cljs::compile_with_macros(recovered, &mut host).unwrap();
        let evaluator = light_cljs::evaluator::Evaluator::new(Default::default()).unwrap();
        assert_eq!(
            evaluator
                .string(evaluator.evaluate_js(&js).unwrap())
                .unwrap(),
            "true"
        );
        assert_eq!(node(&format!("console.log({js});")), "true");
    }
}
