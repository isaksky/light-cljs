//! New complete source arities, with original typed contracts and actual macro demand.
use light_cljs::{MacroHost, compile_namespace_with_macros};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};
const ROWS: &[(&str, &str, &str)] = &[
    (
        "arity-after194-control-nil-bindings",
        include_str!("fixtures/arity-after194/arity-after194-control-nil-bindings.cljs"),
        r###"{"id":"arity-after194-control-nil-bindings","outcome":{"status":"returned","value":true},"trace":[],"stdout":""}"###,
    ),
    (
        "arity-after194-control-binding-scope",
        include_str!("fixtures/arity-after194/arity-after194-control-binding-scope.cljs"),
        r###"{"id":"arity-after194-control-binding-scope","outcome":{"status":"returned","value":true},"trace":[],"stdout":""}"###,
    ),
    (
        "arity-after194-control-binding-once",
        include_str!("fixtures/arity-after194/arity-after194-control-binding-once.cljs"),
        r###"{"id":"arity-after194-control-binding-once","outcome":{"status":"returned","value":true},"trace":[2,3],"stdout":""}"###,
    ),
    (
        "arity-after194-control-when-first",
        include_str!("fixtures/arity-after194/arity-after194-control-when-first.cljs"),
        r###"{"id":"arity-after194-control-when-first","outcome":{"status":"returned","value":true},"trace":[{"$vector":[2,3]}],"stdout":""}"###,
    ),
    (
        "arity-after194-control-macro-contexts",
        include_str!("fixtures/arity-after194/arity-after194-control-macro-contexts.cljs"),
        r###"{"id":"arity-after194-control-macro-contexts","outcome":{"status":"returned","value":true},"trace":[],"stdout":""}"###,
    ),
    (
        "arity-after194-state-constructors",
        include_str!("fixtures/arity-after194/arity-after194-state-constructors.cljs"),
        r###"{"id":"arity-after194-state-constructors","outcome":{"status":"returned","value":{"$vector":[1,2,3,4,5,true,false,true,false,false,false,true,false]}},"trace":[],"stdout":""}"###,
    ),
    (
        "arity-after194-definitions-declare-forward",
        include_str!("fixtures/arity-after194/arity-after194-definitions-declare-forward.cljs"),
        r###"{"id":"arity-after194-definitions-declare-forward","outcome":{"status":"returned","value":true},"trace":[],"stdout":""}"###,
    ),
    (
        "arity-after194-definitions-declare-after",
        include_str!("fixtures/arity-after194/arity-after194-definitions-declare-after.cljs"),
        r###"{"id":"arity-after194-definitions-declare-after","outcome":{"status":"returned","value":true},"trace":[],"stdout":""}"###,
    ),
    (
        "arity-after194-definitions-exists",
        include_str!("fixtures/arity-after194/arity-after194-definitions-exists.cljs"),
        r###"{"id":"arity-after194-definitions-exists","outcome":{"status":"returned","value":true},"trace":[],"stdout":""}"###,
    ),
    (
        "arity-after194-definitions-declare-result",
        include_str!("fixtures/arity-after194/arity-after194-definitions-declare-result.cljs"),
        r###"{"id":"arity-after194-definitions-declare-result","outcome":{"status":"returned","value":true},"trace":[],"stdout":""}"###,
    ),
    (
        "arity-after194-interop-dot-shapes",
        include_str!("fixtures/arity-after194/arity-after194-interop-dot-shapes.cljs"),
        r###"{"id":"arity-after194-interop-dot-shapes","outcome":{"status":"returned","value":true},"trace":[],"stdout":""}"###,
    ),
    (
        "arity-after194-helper-entry-type-changes",
        include_str!("fixtures/arity-after194/arity-after194-helper-entry-type-changes.cljs"),
        r###"{"id":"arity-after194-helper-entry-type-changes","outcome":{"status":"returned","value":true},"trace":[],"stdout":""}"###,
    ),
    (
        "arity-after194-helper-entry-indexes",
        include_str!("fixtures/arity-after194/arity-after194-helper-entry-indexes.cljs"),
        r###"{"id":"arity-after194-helper-entry-indexes","outcome":{"status":"returned","value":true},"trace":[],"stdout":""}"###,
    ),
    (
        "arity-after194-helper-keys-traversal-effects",
        include_str!("fixtures/arity-after194/arity-after194-helper-keys-traversal-effects.cljs"),
        r###"{"id":"arity-after194-helper-keys-traversal-effects","outcome":{"status":"returned","value":true},"trace":["seq","made","next","rested","first","next"],"stdout":""}"###,
    ),
    (
        "arity-after194-helper-vals-traversal-effects",
        include_str!("fixtures/arity-after194/arity-after194-helper-vals-traversal-effects.cljs"),
        r###"{"id":"arity-after194-helper-vals-traversal-effects","outcome":{"status":"returned","value":true},"trace":["seq","made","next","rested","first","next"],"stdout":""}"###,
    ),
    (
        "arity-after194-declare191-original",
        include_str!("fixtures/arity-after194/arity-after194-declare191-original.cljs"),
        r###"{"id":"arity-after194-declare191-original","outcome":{"status":"returned","value":{"$vector":[false,null,null]}},"trace":[],"stdout":""}"###,
    ),
    (
        "arity-after194-declare191-single",
        include_str!("fixtures/arity-after194/arity-after194-declare191-single.cljs"),
        r###"{"id":"arity-after194-declare191-single","outcome":{"status":"returned","value":{"$vector":[true,true,false,false]}},"trace":[],"stdout":""}"###,
    ),
    (
        "arity-after194-declare191-multiple",
        include_str!("fixtures/arity-after194/arity-after194-declare191-multiple.cljs"),
        r###"{"id":"arity-after194-declare191-multiple","outcome":{"status":"returned","value":{"$vector":[true,true,false,false]}},"trace":[],"stdout":""}"###,
    ),
    (
        "arity-after194-declare191-zero",
        include_str!("fixtures/arity-after194/arity-after194-declare191-zero.cljs"),
        r###"{"id":"arity-after194-declare191-zero","outcome":{"status":"returned","value":{"$vector":[false,true,false,true]}},"trace":[],"stdout":""}"###,
    ),
    (
        "arity-after194-declare191-nested",
        include_str!("fixtures/arity-after194/arity-after194-declare191-nested.cljs"),
        r###"{"id":"arity-after194-declare191-nested","outcome":{"status":"returned","value":{"$vector":[true,true,true,null]}},"trace":[{"$keyword":{"namespace":null,"name":"before","spelling":"before"}}],"stdout":""}"###,
    ),
    (
        "arity-after194-declare191-callback",
        include_str!("fixtures/arity-after194/arity-after194-declare191-callback.cljs"),
        r###"{"id":"arity-after194-declare191-callback","outcome":{"status":"returned","value":{"$vector":[{"$vector":[{"$value":"undefined"},{"$value":"undefined"}]},{"$vector":[null,null]}]}},"trace":[1,2,3,4],"stdout":""}"###,
    ),
    (
        "arity-after194-declare191-redefinition",
        include_str!("fixtures/arity-after194/arity-after194-declare191-redefinition.cljs"),
        r###"{"id":"arity-after194-declare191-redefinition","outcome":{"status":"returned","value":{"$vector":[true,3,2,null]}},"trace":[],"stdout":""}"###,
    ),
    (
        "arity-after194-sequential-conj-arities",
        include_str!("fixtures/arity-after194/arity-after194-sequential-conj-arities.cljs"),
        r###"{"id":"arity-after194-sequential-conj-arities","outcome":{"status":"returned","value":{"$vector":[{"$vector":[]},{"$vector":[1]},{"$vector":[1,2,3]},{"$list":[3,2,1]},{"$list":[2,1]}]}},"trace":[],"stdout":""}"###,
    ),
    (
        "arity-after194-deferred-lazy-constructors",
        include_str!("fixtures/arity-after194/arity-after194-deferred-lazy-constructors.cljs"),
        r###"{"id":"arity-after194-deferred-lazy-constructors","outcome":{"status":"returned","value":true},"trace":[],"stdout":""}"###,
    ),
    (
        "arity-after194-deferred-deferred-constructor-false",
        include_str!(
            "fixtures/arity-after194/arity-after194-deferred-deferred-constructor-false.cljs"
        ),
        r###"{"id":"arity-after194-deferred-deferred-constructor-false","outcome":{"status":"returned","value":true},"trace":[],"stdout":""}"###,
    ),
    (
        "arity-after194-partitionv-all",
        include_str!("fixtures/arity-after194/arity-after194-partitionv-all.cljs"),
        r###"{"id":"arity-after194-partitionv-all","outcome":{"status":"returned","value":[["returned","[[[1 2] [3]] [[[1 2] [3]] [[4 5]]]]"],"[]"]},"trace":[],"stdout":""}"###,
    ),
    (
        "arity-after194-cljs-utilities-completion-primitives-test-test-make-array",
        include_str!(
            "fixtures/arity-after194/arity-after194-cljs-utilities-completion-primitives-test-test-make-array.cljs"
        ),
        r###"{"id":"arity-after194-cljs-utilities-completion-primitives-test-test-make-array","outcome":{"status":"returned","value":true},"trace":[],"stdout":""}"###,
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
fn remaining_whole_arity_sources_execute_in_actual_phases_and_lifecycles() {
    let artifacts = std::env::var_os("LIGHT_ARITY_AFTER194_ARTIFACTS")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!("light-arity-after194-{}", std::process::id()))
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
                    include_str!("support/sci_execution_observation.js")
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
                                "arity-after194-compile-error\t{id}\t{lifecycle}\t{phase}\t{e}"
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
                        include_str!("support/sci_execution_observation.js")
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
                    "arity-after194-observation\t{id}\t{lifecycle}\t{phase}\tQuickJS\t{quickjs}"
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
                println!("arity-after194-artifacts\t{id}\t{lifecycle}\t{phase}\t{hashes}");
                assert_eq!(actual, expected, "{id} {lifecycle} {phase} Node");
                println!("arity-after194-observation\t{id}\t{lifecycle}\t{phase}\tNode\t{actual}");
            }
        }
    }
}
