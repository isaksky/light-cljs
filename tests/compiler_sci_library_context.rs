//! Complete SCI library bodies with explicit original user/import fixture.
use std::{
    fs,
    io::Write,
    process::{Command, Stdio},
};

fn unhex(text: &str) -> String {
    assert_eq!(text.len() % 2, 0);
    String::from_utf8(
        text.as_bytes()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect(),
    )
    .unwrap()
}

fn script(
    id: &str,
    declarations: &str,
    source: &str,
    root: &std::path::Path,
) -> Result<String, String> {
    fs::create_dir_all(root).unwrap();
    // Original require declarations stay top-level. The full remaining source
    // body is inside the same visible observation function used by the oracle.
    let fixture = format!(
        r#"(ns user (:require [clojure.string] [clojure.set] [clojure.walk] [clojure.edn]))
{declarations}
{}
(fn* [observe]
 (set! *print-newline* true)
 (set-print-fn! (fn [text] (js/globalThis.lightLibraryWrite text)))
 (set-print-err-fn! (fn [text] (js/globalThis.lightLibraryWriteErr text)))
 {source})"#,
        include_str!("fixtures/contracts/sci_library_inspection.cljs")
    );
    fs::write(root.join("user.cljs"), fixture).unwrap();
    let compiled = light_cljs::compile_namespace_with_macros(
        "user",
        &[root.to_path_buf()],
        &mut light_cljs::MacroHost::new(),
    )
    .map_err(|error| format!("{error:?}"))?;
    Ok(format!(
        "(() => {{ const run = {compiled}; const rt = {{inspect: globalThis.lightLibraryInspect, core: {{isSequential: () => false}}}}; return ({})(rt)('{id}', observe => {{ globalThis.lightLibraryWrite = rt.core.printFn; globalThis.lightLibraryWriteErr = rt.core.printErrFn; return run(observe); }}); }})()",
        include_str!("support/sci_execution_observation.js")
    ))
}

fn execute(script: &str) -> (String, String) {
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
    let node = if result.status.success() {
        String::from_utf8(result.stdout).unwrap().trim_end().into()
    } else {
        format!(
            "HARNESS:{}",
            String::from_utf8_lossy(&result.stderr).replace(['\n', '\t'], " ")
        )
    };
    let runtime = rquickjs::Runtime::new().unwrap();
    runtime.set_memory_limit(128 * 1024 * 1024);
    runtime.set_max_stack_size(4 * 1024 * 1024);
    let context = rquickjs::Context::full(&runtime).unwrap();
    let quickjs = context.with(|ctx| match ctx.eval::<String, _>(script.as_bytes()) {
        Ok(value) => value,
        Err(error) => format!("HARNESS:{error:?}: {:?}", ctx.catch()).replace(['\n', '\t'], " "),
    });
    (node, quickjs)
}

fn execute_case(line: &str, root: &std::path::Path) -> String {
    let fields: Vec<_> = line.split('\t').collect();
    assert_eq!(fields.len(), 3);
    assert!(
        fields[0]
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-')
    );
    match script(
        fields[0],
        &unhex(fields[1]),
        &unhex(fields[2]),
        &root.join(fields[0]),
    ) {
        Ok(script) => {
            let (node, quickjs) = execute(&script);
            format!("{}\texecuted\t{node}\t{quickjs}", fields[0])
        }
        Err(error) => format!(
            "{}\trejected\t{}",
            fields[0],
            error.replace(['\n', '\t'], " ")
        ),
    }
}

#[test]
fn library_import_fixture_preserves_values_and_callbacks_in_both_engines() {
    let root =
        std::env::temp_dir().join(format!("light-cljs-library-bridge-{}", std::process::id()));
    let fixtures = [
        (
            "string",
            "(require '[clojure.string :as string])",
            "(string/join \"-\" [1 2 3])",
            r#"{"id":"string","outcome":{"status":"returned","value":"1-2-3"},"trace":[],"stdout":""}"#,
        ),
        (
            "set",
            "",
            "(clojure.set/select (fn [v] (observe v) false) #{9})",
            r#"{"id":"set","outcome":{"status":"returned","value":{"$set":[],"count":0}},"trace":[9],"stdout":""}"#,
        ),
        (
            "walk",
            "",
            "(clojure.walk/walk (fn [x] (observe :inner) x) (fn [x] (observe :outer) (inc x)) 1)",
            r#"{"id":"walk","outcome":{"status":"returned","value":2},"trace":[{"$keyword":{"namespace":null,"name":"outer","spelling":"outer"}}],"stdout":""}"#,
        ),
        (
            "edn",
            "",
            "(clojure.edn/read-string {:readers {'tag inc} :default (fn [t v] (observe :bad) 0)} \"#tag 1\")",
            r#"{"id":"edn","outcome":{"status":"returned","value":2},"trace":[],"stdout":""}"#,
        ),
    ];
    std::thread::scope(|scope| {
        let handles: Vec<_> = fixtures
            .into_iter()
            .map(|(id, declarations, source, expected)| {
                let root = &root;
                scope.spawn(move || {
                    let script = script(id, declarations, source, &root.join(id)).unwrap();
                    let (node, quickjs) = execute(&script);
                    assert_eq!(node, expected, "Node {id}");
                    assert_eq!(quickjs, expected, "QuickJS {id}");
                })
            })
            .collect();
        for handle in handles {
            handle.join().unwrap();
        }
    });
    fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "explicit complete SCI library/import original-contract audit"]
fn retained_library_sources_execute_in_node_and_quickjs() {
    let input = std::env::var("LIGHT_CLJS_SCI_LIBRARY_INPUT").expect("source TSV");
    let output = std::env::var("LIGHT_CLJS_SCI_LIBRARY_OUTPUT").expect("observation TSV");
    let root = std::path::Path::new(&output)
        .parent()
        .unwrap()
        .join("sources");
    let mut out = fs::File::create(output).unwrap();
    let input = fs::read_to_string(input).unwrap();
    let lines: Vec<_> = input.lines().collect();
    // Each worker owns a fresh MacroHost, Node process and QuickJS heap per
    // original case. Joining in input order preserves the raw receipt protocol.
    // A single worker is available for literal serial/parallel fixture audits.
    let workers = std::env::var("LIGHT_CLJS_SCI_LIBRARY_WORKERS")
        .ok()
        .map(|value| value.parse::<usize>().expect("worker count"))
        .unwrap_or(4);
    assert!((1..=4).contains(&workers), "one through four workers");
    for batch in lines.chunks(workers) {
        let observations = std::thread::scope(|scope| {
            let handles: Vec<_> = batch
                .iter()
                .map(|line| {
                    let root = &root;
                    scope.spawn(move || execute_case(line, root))
                })
                .collect();
            handles
                .into_iter()
                .map(|handle| handle.join().unwrap())
                .collect::<Vec<_>>()
        });
        for observation in observations {
            writeln!(out, "{observation}").unwrap();
        }
    }
    assert!(!lines.is_empty(), "zero source executions");
}
