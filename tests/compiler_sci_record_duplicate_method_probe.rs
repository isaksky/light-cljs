//! Exact original and literal two-pin method replacement observations.
use light_cljs::MacroHost;
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};

const ORIGINAL: &str =
    "(defprotocol P (p [x])) (defrecord R [x] P (p [_] x) (p [_] 9)) (p (->R 3))";
const PRESERVATION: &str = "(def events (atom [])) (defprotocol P (p [x] [x n]) (q [x])) (defrecord R [v] P (p [_] (swap! events conj :discarded) 0) (p [_ n] (+ v n)) (q [_] (* v 2)) (p [_] (swap! events conj v) v)) (def old (->R 3)) (def current (->R 7)) [(p old) (p current) (p old 4) (q current) @events]";
const WARMUP: &str = "(defmacro ready [] true) (ready)";
const GENERATIONS: &str = "(defprotocol P (p [x])) (defrecord R [x] P (p [_] 0) (p [_] x)) (def old (->R 3)) (def factory ->R) (defrecord R [x] P (p [_] 0) (p [_] (+ 10 x))) (def current (->R 7)) [(p old) (p current) (p (factory 5)) (p (assoc old :x 4))]";

fn execute(javascript: &str, expected_value: &str, directory: &std::path::Path) {
    let captured = javascript.replacen(
        " = // Original bounded runtime",
        " = globalThis.lightRuntime = // Original bounded runtime",
        1,
    );
    let script = format!(
        "(() => {{ const run = {captured}; return ({}) (globalThis.lightRuntime)('duplicate-method',observe=>run(observe)); }})()",
        include_str!("support/sci_execution_observation.js")
    );
    let expected = format!(
        r#"{{"id":"duplicate-method","outcome":{{"status":"returned","value":{expected_value}}},"trace":[],"stdout":""}}"#
    );
    let runtime = rquickjs::Runtime::new().unwrap();
    let context = rquickjs::Context::full(&runtime).unwrap();
    let quickjs = context.with(|ctx| ctx.eval::<String, _>(script.as_bytes()).unwrap());
    fs::write(directory.join("quickjs.json"), &quickjs).unwrap();
    assert_eq!(quickjs, expected);
    let mut node = Command::new(std::env::var_os("NODE").unwrap_or_else(|| "node".into()))
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    node.stdin
        .take()
        .unwrap()
        .write_all(format!("console.log({script});").as_bytes())
        .unwrap();
    let result = node.wait_with_output().unwrap();
    fs::write(directory.join("node.stdout"), &result.stdout).unwrap();
    fs::write(directory.join("node.stderr"), &result.stderr).unwrap();
    assert!(
        result.status.success() && result.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(String::from_utf8(result.stdout).unwrap(), expected + "\n");
}

#[test]
fn original_duplicate_method_executes_with_last_dispatch_in_both_phases() {
    let retained = std::env::var_os("LIGHT_SCI_RECORD_DUPLICATE_METHOD_ROOT");
    let root = retained.as_ref().map(PathBuf::from).unwrap_or_else(|| {
        std::env::temp_dir().join(format!(
            "light-cljs-record-last-method-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    });
    assert!(!root.exists());
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("original.cljs"), ORIGINAL).unwrap();
    let mut reused = MacroHost::new();
    fs::write(
        root.join("warmup.js"),
        light_cljs::compile_with_macros(WARMUP, &mut reused).unwrap(),
    )
    .unwrap();
    for (case, original, expected) in [
        ("original", ORIGINAL, "9"),
        (
            "fields-arities-effects",
            PRESERVATION,
            r#"{"$vector":[3,7,7,14,{"$vector":[3,7]}]}"#,
        ),
        (
            "deftype",
            "(defprotocol P (p [x])) (deftype R [x] P (p [_] x) (p [_] 9)) (p (->R 3))",
            "9",
        ),
        (
            "reify",
            "(defprotocol P (p [x])) (p (reify P (p [_] 3) (p [_] 9)))",
            "9",
        ),
        (
            "generation-identity-effects",
            "(def events (atom [])) (defprotocol P (p [x])) (defrecord R [x] P (p [_] x)) (def old (do (swap! events conj :old) (->R 3))) (def ctor R) (def factory ->R) (def map-factory map->R) (defrecord R [x] P (p [_] (+ 10 x))) (def current (do (swap! events conj :current) (->R 7))) [(p old) (p old) (p current) (p current) (identical? ctor R) (instance? ctor old) (instance? R old) (identical? factory ->R) (p (factory 5)) (p (map-factory {:x 6})) (p (assoc old :x 4)) (:x old) @events]",
            r#"{"$vector":[3,3,17,17,false,true,false,false,15,16,14,3,{"$vector":[{"$keyword":{"namespace":null,"name":"old","spelling":"old"}},{"$keyword":{"namespace":null,"name":"current","spelling":"current"}}]}]}"#,
        ),
        (
            "nonduplicate-generations",
            "(defprotocol P (p [x])) (defrecord R [x] P (p [_] x)) (def old (->R 3)) (def factory ->R) (defrecord R [x] P (p [_] (+ 10 x))) (def current (->R 7)) [(p old) (p current) (p (factory 5)) (p (assoc old :x 4))]",
            r#"{"$vector":[3,17,15,14]}"#,
        ),
        ("generations", GENERATIONS, r#"{"$vector":[3,17,15,14]}"#),
    ] {
        let syntax = light_cljs::parse(original).unwrap();
        let last = syntax.nodes[syntax.roots.last().unwrap().0].span.clone();
        let (setup, body) = (&original[..last.start], &original[last]);
        for reuse in [false, true] {
            for phase in ["application", "top-level-macro", "complete-macro"] {
                let source = match phase {
                    "application" => format!("(ns user)\n{setup}(defn run [] {body})\n"),
                    "top-level-macro" => format!(
                        "(ns user)\n{setup}(defmacro upstream [] (list 'quote {body}))\n(defn run [] (upstream))\n"
                    ),
                    _ => format!(
                        "(ns user)\n(defmacro upstream [] (list 'quote (do {original})))\n(defn run [] (upstream))\n"
                    ),
                };
                let name = format!("{case}-{phase}-{}", if reuse { "reused" } else { "fresh" });
                let directory = root.join(&name);
                fs::create_dir(&directory).unwrap();
                fs::write(directory.join("user.cljs"), &source).unwrap();
                let mut fresh = MacroHost::new();
                let host = if reuse { &mut reused } else { &mut fresh };
                let javascript = light_cljs::compile_namespace_with_macros(
                    "user",
                    std::slice::from_ref(&directory),
                    host,
                )
                .unwrap_or_else(|e| panic!("{name}: {e}"));
                fs::write(directory.join("compiled.js"), &javascript).unwrap();
                execute(&javascript, expected, &directory);
                // Compilation after publication and located malformed-method failure
                // must recover in the same host, including an actually warmed host.
                let error = light_cljs::compile_with_macros(
                    "(defprotocol Invalid (f [x])) (defrecord Bad [x] Invalid (f [] 0))",
                    host,
                )
                .unwrap_err()
                .to_string();
                assert!(
                    error.contains("protocol method arity mismatch"),
                    "{name}: {error}"
                );
                fs::write(directory.join("error.txt"), error).unwrap();
                light_cljs::compile_with_macros("(defmacro clean [] true) (clean)", host).unwrap();
                println!("{name}: QuickJS/Node literal result {expected}; recovery succeeded");
            }
        }
    }
    if retained.is_none() {
        fs::remove_dir_all(root).unwrap();
    }
}
