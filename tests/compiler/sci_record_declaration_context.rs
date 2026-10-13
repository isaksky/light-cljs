//! Three exact original declaration replacement/marker sources with independently pinned dialect results.
//! Original SCI expectations remain immutable and unresolved.
use light_cljs::MacroHost;
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};
const ORIGINALS: [(&str, &str); 3] = [
    (
        "sci-execution-sci-record-redefine",
        "(defrecord R [x]) (def old (->R 1)) (def saved ->R) (def saved-map map->R) (defrecord R [x]) [(= old (->R 1)) (= (saved 1) (->R 1)) (= (saved-map {:x 1}) (->R 1))]",
    ),
    (
        "sci-execution-sci-record-method-same-shape",
        "(defprotocol P (p [x])) (defrecord R [x] P (p [_] x)) (def r (->R 2)) (def f p) (defprotocol P (p [x])) [(= f p) (satisfies? P r) (f r) (p r)]",
    ),
    (
        "sci-execution-sci-protocol-marker-return",
        "[(= 'Marker (defprotocol Marker)) (satisfies? Marker nil) (satisfies? Marker {})]",
    ),
];
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
fn original_record_declarations_execute_in_both_phases_and_lifecycles() {
    let input =
        fs::read_to_string(std::env::var("LIGHT_SCI_RECORD_DECLARATION_INPUT").unwrap()).unwrap();
    let rows: Vec<_> = input.lines().collect();
    assert_eq!(rows.len(), ORIGINALS.len());
    let root = PathBuf::from(std::env::var("LIGHT_SCI_RECORD_DECLARATION_ROOT").unwrap());
    assert!(!root.exists());
    let mut output =
        fs::File::create(std::env::var("LIGHT_SCI_RECORD_DECLARATION_OUTPUT").unwrap()).unwrap();
    let mut reused = MacroHost::new();
    let warmup = light_cljs::compile_with_macros(WARMUP, &mut reused).unwrap();
    for ((id, original), row) in ORIGINALS.into_iter().zip(rows) {
        let fields: Vec<_> = row.split('\t').collect();
        assert_eq!(fields.len(), 3);
        assert_eq!(fields[0], id);
        assert_eq!(unhex(fields[1]), original);
        let expected = unhex(fields[2]);
        let syntax = light_cljs::parse(original).unwrap();
        let last = syntax.nodes[syntax.roots.last().unwrap().0].span.clone();
        let setup = &original[..last.start];
        let body = &original[last];
        for (phase, reuse) in [
            ("application", false),
            ("macro", false),
            ("application", true),
            ("macro", true),
        ] {
            let variant = format!("{phase}-{}", if reuse { "reused" } else { "fresh" });
            let source = if phase == "macro" {
                format!(
                    "(ns user)\n(defmacro upstream [] (list 'quote (do {original})))\n(defn run [] (upstream))\n"
                )
            } else {
                format!("(ns user)\n{setup}(defn run [] {body})\n")
            };
            let variantroot = root.join(id).join(&variant);
            fs::create_dir_all(&variantroot).unwrap();
            fs::write(variantroot.join("user.cljs"), &source).unwrap();
            let mut fresh = MacroHost::new();
            let javascript = light_cljs::compile_namespace_with_macros(
                "user",
                &[variantroot],
                if reuse { &mut reused } else { &mut fresh },
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
                "(() => {{ const run = {captured}; return ({}) (globalThis.lightRuntime)('{id}',observe=>run(observe)); }})()",
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
                    "{id}\t{variant}\t{engine}\t{}\t{}\t{}\t{}",
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
}
