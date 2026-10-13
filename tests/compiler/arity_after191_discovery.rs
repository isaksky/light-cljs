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
        "arity-after191-discovery-invocation-apply-to",
        include_str!(
            "../fixtures/arity-after191-discovery/arity-after191-discovery-invocation-apply-to.cljs"
        ),
        r###"{"id":"arity-after191-discovery-invocation-apply-to","outcome":{"status":"returned","value":true},"trace":[],"stdout":""}"###,
    ),
    (
        "arity-after191-discovery-state-watch-protocol-returns",
        include_str!(
            "../fixtures/arity-after191-discovery/arity-after191-discovery-state-watch-protocol-returns.cljs"
        ),
        r###"{"id":"arity-after191-discovery-state-watch-protocol-returns","outcome":{"status":"returned","value":{"$vector":[true,true,true,true]}},"trace":[],"stdout":""}"###,
    ),
    (
        "arity-after191-discovery-state-constructors",
        include_str!(
            "../fixtures/arity-after191-discovery/arity-after191-discovery-state-constructors.cljs"
        ),
        r###"{"id":"arity-after191-discovery-state-constructors","outcome":{"status":"returned","value":{"$vector":[1,2,3,4,5,true,false,true,false,false,false,true,false]}},"trace":[],"stdout":""}"###,
    ),
    (
        "arity-after191-discovery-interop-object-helpers",
        include_str!(
            "../fixtures/arity-after191-discovery/arity-after191-discovery-interop-object-helpers.cljs"
        ),
        r###"{"id":"arity-after191-discovery-interop-object-helpers","outcome":{"status":"returned","value":true},"trace":[],"stdout":""}"###,
    ),
    (
        "arity-after191-discovery-interop-readonly-write",
        include_str!(
            "../fixtures/arity-after191-discovery/arity-after191-discovery-interop-readonly-write.cljs"
        ),
        r###"{"id":"arity-after191-discovery-interop-readonly-write","outcome":{"status":"returned","value":true},"trace":[],"stdout":""}"###,
    ),
    (
        "arity-after191-discovery-interop-checked-strict",
        include_str!(
            "../fixtures/arity-after191-discovery/arity-after191-discovery-interop-checked-strict.cljs"
        ),
        r###"{"id":"arity-after191-discovery-interop-checked-strict","outcome":{"status":"returned","value":true},"trace":[],"stdout":""}"###,
    ),
    (
        "arity-after191-discovery-protocol-membership",
        include_str!(
            "../fixtures/arity-after191-discovery/arity-after191-discovery-protocol-membership.cljs"
        ),
        r###"{"id":"arity-after191-discovery-protocol-membership","outcome":{"status":"returned","value":true},"trace":[],"stdout":""}"###,
    ),
    (
        "arity-after191-discovery-protocol-metadata-precedence",
        include_str!(
            "../fixtures/arity-after191-discovery/arity-after191-discovery-protocol-metadata-precedence.cljs"
        ),
        r###"{"id":"arity-after191-discovery-protocol-metadata-precedence","outcome":{"status":"returned","value":true},"trace":[],"stdout":""}"###,
    ),
    (
        "arity-after191-discovery-protocol-native-and-default",
        include_str!(
            "../fixtures/arity-after191-discovery/arity-after191-discovery-protocol-native-and-default.cljs"
        ),
        r###"{"id":"arity-after191-discovery-protocol-native-and-default","outcome":{"status":"returned","value":true},"trace":[],"stdout":""}"###,
    ),
    (
        "arity-after191-discovery-protocol-external-named",
        include_str!(
            "../fixtures/arity-after191-discovery/arity-after191-discovery-protocol-external-named.cljs"
        ),
        r###"{"id":"arity-after191-discovery-protocol-external-named","outcome":{"status":"returned","value":true},"trace":[],"stdout":""}"###,
    ),
    (
        "arity-after191-discovery-protocol-external-host",
        include_str!(
            "../fixtures/arity-after191-discovery/arity-after191-discovery-protocol-external-host.cljs"
        ),
        r###"{"id":"arity-after191-discovery-protocol-external-host","outcome":{"status":"returned","value":true},"trace":[],"stdout":""}"###,
    ),
    (
        "arity-after191-discovery-extension-direct-2",
        include_str!(
            "../fixtures/arity-after191-discovery/arity-after191-discovery-extension-direct-2.cljs"
        ),
        r###"{"id":"arity-after191-discovery-extension-direct-2","outcome":{"status":"returned","value":[true,"instance","type","record","metadata",true,true]},"trace":[],"stdout":""}"###,
    ),
    (
        "arity-after191-discovery-extension-instance-object-ifn",
        include_str!(
            "../fixtures/arity-after191-discovery/arity-after191-discovery-extension-instance-object-ifn.cljs"
        ),
        r###"{"id":"arity-after191-discovery-extension-instance-object-ifn","outcome":{"status":"returned","value":true},"trace":[],"stdout":""}"###,
    ),
    (
        "arity-after191-discovery-extension-instance-exception",
        include_str!(
            "../fixtures/arity-after191-discovery/arity-after191-discovery-extension-instance-exception.cljs"
        ),
        r###"{"id":"arity-after191-discovery-extension-instance-exception","outcome":{"status":"returned","value":true},"trace":[],"stdout":""}"###,
    ),
    (
        "arity-after191-discovery-extension-primitive-instance-noop",
        include_str!(
            "../fixtures/arity-after191-discovery/arity-after191-discovery-extension-primitive-instance-noop.cljs"
        ),
        r###"{"id":"arity-after191-discovery-extension-primitive-instance-noop","outcome":{"status":"returned","value":1},"trace":[],"stdout":""}"###,
    ),
    (
        "arity-after191-discovery-core-protocol-writer-flush",
        include_str!(
            "../fixtures/arity-after191-discovery/arity-after191-discovery-core-protocol-writer-flush.cljs"
        ),
        r###"{"id":"arity-after191-discovery-core-protocol-writer-flush","outcome":{"status":"returned","value":true},"trace":[],"stdout":""}"###,
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
fn complete_disjoint_sources_are_observed_in_actual_phases_and_lifecycles() {
    let artifacts = std::env::var_os("LIGHT_ARITY_AFTER191_DISCOVERY_ARTIFACTS")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!(
                "light-arity-after191-discovery-{}",
                std::process::id()
            ))
        });
    fs::create_dir_all(&artifacts).unwrap();
    let runtime = light_cljs::compile("[]").unwrap();
    fs::write(artifacts.join("observer-runtime.js"), &runtime).unwrap();
    let mut reused = host();
    let warmup = "(defmacro arity-ready [] (+ 20 22)) (arity-ready)";
    let warmup_js = light_cljs::compile_with_macros(warmup, &mut reused).unwrap();
    fs::write(artifacts.join("warmup.cljs"), warmup).unwrap();
    fs::write(artifacts.join("warmup.js"), &warmup_js).unwrap();
    for &(id, source, _expected) in ROWS {
        for lifecycle in ["fresh", "reused"] {
            for phase in ["application", "macro"] {
                let program = if phase == "macro" {
                    format!(
                        "(ns user)\n(defmacro arity-witness [] (list 'quote (do {source})))\n(defn run [observe] (arity-witness))\nrun\n"
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
                let js = match compile_namespace_with_macros(
                    "user",
                    std::slice::from_ref(&root),
                    actual_host,
                ) {
                    Ok(js) => js,
                    Err(error) => {
                        fs::write(root.join("rejection.txt"), error.to_string()).unwrap();
                        println!(
                            "arity-after191-discovery-rejection\t{id}\t{lifecycle}\t{phase}\t{error}"
                        );
                        continue;
                    }
                };
                fs::write(root.join("compiled.js"), &js).unwrap();
                let script = format!(
                    "(() => {{void {};const run={};return ({})(globalThis.lightRuntime)('{id}',observe=>run(observe));}})()",
                    capture(&runtime),
                    capture(&js),
                    include_str!("../support/sci_execution_observation.js")
                );
                fs::write(root.join("script.js"), &script).unwrap();
                let runtime = rquickjs::Runtime::new().unwrap();
                let context = rquickjs::Context::full(&runtime).unwrap();
                let quickjs = context.with(|ctx| {
                    ctx.eval::<String, _>(format!("(0, eval)({script:?})").as_bytes())
                        .unwrap_or_else(|e| {
                            panic!("{id} {lifecycle} {phase} observer: {e:?} {:?}", ctx.catch())
                        })
                });
                fs::write(root.join("quickjs.stdin"), format!("(0, eval)({script:?})")).unwrap();
                fs::write(root.join("quickjs.raw"), &quickjs).unwrap();
                println!(
                    "arity-after191-discovery-observation\t{id}\t{lifecycle}\t{phase}\tQuickJS\t{quickjs}"
                );
                let input = format!("console.log({script});");
                fs::write(root.join("node.stdin"), &input).unwrap();
                let packet = node(&format!(
                    "const sha=x=>require('node:crypto').createHash('sha256').update(x).digest('hex');console.log(sha({js:?})+'\\t'+sha({program:?})+'\\t'+sha({script:?}));console.log({script});"
                ));
                let (hashes, actual) = packet
                    .split_once('\n')
                    .expect("actual Node input hash packet");
                println!(
                    "arity-after191-discovery-artifacts\t{id}\t{lifecycle}\t{phase}\t{hashes}"
                );
                fs::write(root.join("node.stdout"), format!("{actual}\n")).unwrap();
                println!(
                    "arity-after191-discovery-observation\t{id}\t{lifecycle}\t{phase}\tNode\t{actual}"
                );
            }
        }
    }
}
