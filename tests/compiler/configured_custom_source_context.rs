//! Complete original custom-reader CLJC module and original reader configuration.
//! Explicit compile-reader branch; no general JVM companion/library parity.
use light_cljs::{MacroHost, compiler_data::Name};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};
const ID: &str = "cljs-original-configured-custom-reader-context";
const ORIGINAL: &str =
    include_str!("../../research/clojurescript/src/test/cljs/data_readers_test/core.cljc");
const CONFIG: &str = include_str!("../../research/clojurescript/src/test/cljs/data_readers.cljc");
const RESULT_CHECK: &str = "(assert (and (array? result) (= [\"foo\"] (vec result))))";
const COMPILE_FORM_READER: &str = "(fn [x] `(js/Array.of ~x))";
const WARMUP: &str = "(defmacro phase-ready [] true) (phase-ready)";
fn hex(value: &str) -> String {
    value
        .as_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn unhex(value: &str) -> String {
    assert_eq!(value.len() % 2, 0);
    String::from_utf8(
        value
            .as_bytes()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
            .collect(),
    )
    .unwrap()
}
fn canonical(context: &rquickjs::Context, value: &str) -> String {
    let script = format!(
        "JSON.stringify((function sort(v) {{ if(Array.isArray(v)) return v.map(sort); if(v && typeof v==='object') {{ const result={{}}; Object.keys(v).sort().forEach(k=>result[k]=sort(v[k])); return result; }} return v; }})(JSON.parse({value:?})))"
    );
    context.with(|ctx| ctx.eval::<String, _>(script.as_bytes()).unwrap())
}

#[test]
#[ignore = "Explicit exact original source and current both-pin provenance helper"]
fn complete_custom_reader_source_executes_in_both_phases_and_lifecycles() {
    let input =
        fs::read_to_string(std::env::var("LIGHT_CONFIGURED_CUSTOM_SOURCE_INPUT").unwrap()).unwrap();
    let rows: Vec<_> = input.lines().collect();
    assert_eq!(rows.len(), 1);
    let fields: Vec<_> = rows[0].split('\t').collect();
    assert_eq!(fields.len(), 4);
    assert_eq!(fields[0], ID);
    assert_eq!(unhex(fields[1]), ORIGINAL);
    assert_eq!(unhex(fields[2]), CONFIG);
    let expected = unhex(fields[3]);
    let root = PathBuf::from(std::env::var("LIGHT_CONFIGURED_CUSTOM_SOURCE_ROOT").unwrap());
    assert!(!root.exists());
    let mut output =
        fs::File::create(std::env::var("LIGHT_CONFIGURED_CUSTOM_SOURCE_OUTPUT").unwrap()).unwrap();
    let mut reused = MacroHost::new();
    let warmup = light_cljs::compile_with_macros(WARMUP, &mut reused).unwrap();
    for (phase, reuse) in [
        ("application", false),
        ("macro", false),
        ("application", true),
        ("macro", true),
    ] {
        let variant = format!("{phase}-{}", if reuse { "reused" } else { "fresh" });
        let source = if phase == "macro" {
            let (prefix, body) = ORIGINAL.split_once("(ns data-readers-test.core)").unwrap();
            format!(
                "{prefix}(ns data-readers-test.core)\n(defmacro upstream [] (do {body} {RESULT_CHECK} true))\n(defn run [] (upstream))\n"
            )
        } else {
            format!("{ORIGINAL}\n(defn run [] (do {RESULT_CHECK} true))\n")
        };
        let variantroot = root.join(&variant);
        fs::create_dir_all(variantroot.join("data_readers_test")).unwrap();
        fs::write(variantroot.join("data_readers_test/core.cljc"), &source).unwrap();
        let mut fresh = MacroHost::new();
        let host = if reuse { &mut reused } else { &mut fresh };
        host.register_compiler_data_reader(Name::new(Some("test"), "custom-identity"), "identity")
            .unwrap();
        host.register_compiler_data_reader(
            Name::new(Some("test"), "custom-form"),
            COMPILE_FORM_READER,
        )
        .unwrap();
        let javascript = light_cljs::compile_namespace_with_macros(
            "data-readers-test.core",
            &[variantroot],
            host,
        )
        .unwrap();
        let anchor = " = // Original bounded runtime";
        assert_eq!(javascript.matches(anchor).count(), 1);
        let captured = javascript.replacen(
            anchor,
            " = globalThis.lightRuntime = // Original bounded runtime",
            1,
        );
        let script = format!(
            "(() => {{ const run = {captured}; return ({}) (globalThis.lightRuntime)('{ID}',observe=>run(observe)); }})()",
            include_str!("../support/sci_execution_observation.js")
        );
        let runtime = rquickjs::Runtime::new().unwrap();
        runtime.set_memory_limit(128 * 1024 * 1024);
        runtime.set_max_stack_size(4 * 1024 * 1024);
        let context = rquickjs::Context::full(&runtime).unwrap();
        let quickjs = context.with(|ctx| ctx.eval::<String, _>(script.as_bytes()).unwrap());
        assert_eq!(
            canonical(&context, &quickjs),
            canonical(&context, &expected),
            "{variant} QuickJS"
        );
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
        assert!(result.status.success());
        assert!(result.stderr.is_empty());
        let node = String::from_utf8(result.stdout).unwrap();
        assert_eq!(
            canonical(&context, &node),
            canonical(&context, &expected),
            "{variant} Node"
        );
        for (engine, raw) in [("QuickJS", quickjs), ("Node", node)] {
            writeln!(
                output,
                "{variant}\t{engine}\t{}\t{}\t{}\t{}",
                hex(&source),
                hex(&javascript),
                hex(&raw),
                if reuse { hex(&warmup) } else { "-".into() }
            )
            .unwrap();
            output.flush().unwrap();
        }
    }
}
