//! Explicit original-source diagnostic audit; observations do not grant completion credit.
use light_cljs::MacroHost;
use std::{
    fs,
    io::Write,
    process::{Command, Stdio},
};
fn hex(text: &str) -> String {
    text.bytes().map(|byte| format!("{byte:02x}")).collect()
}
fn unhex(text: &str) -> String {
    String::from_utf8(
        text.as_bytes()
            .as_chunks::<2>().0.iter()
            .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
            .collect(),
    )
    .unwrap()
}
fn observe(script: &str) -> [(String, String); 2] {
    let runtime = rquickjs::Runtime::new().unwrap();
    let context = rquickjs::Context::full(&runtime).unwrap();
    let quickjs = context.with(|ctx| ctx.eval::<String, _>(script.as_bytes()).unwrap());
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
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(result.stderr.is_empty());
    [
        ("QuickJS".into(), quickjs),
        (
            "Node".into(),
            String::from_utf8(result.stdout).unwrap().trim().into(),
        ),
    ]
}
#[test]
#[ignore = "Explicit unchanged source applications and demanded macro observation audit"]
fn original_gap_sources_execute_through_actual_macro_hosts() {
    let root = std::path::PathBuf::from(std::env::var("LIGHT_SCI_GAP_PHASE_ROOT").unwrap());
    let input = fs::read_to_string(root.join("input.tsv")).unwrap();
    let mut output = fs::File::create(root.join("observations.tsv")).unwrap();
    let mut reused = MacroHost::new();
    let warmup = light_cljs::compile_with_macros(
        "(defmacro phase-ready [] (+ 20 22)) (phase-ready)",
        &mut reused,
    )
    .unwrap();
    fs::write(root.join("warmup.js"), &warmup).unwrap();
    let mut count = 0;
    for row in input.lines() {
        let fields: Vec<_> = row.split('\t').collect();
        assert_eq!(2, fields.len());
        let id = fields[0];
        let original = unhex(fields[1]);
        for reuse in [false, true] {
            for phase in [false, true] {
                let variant = format!(
                    "{}-{}",
                    if phase { "macro" } else { "application" },
                    if reuse { "reused" } else { "fresh" }
                );
                let source = if phase {
                    format!(
                        "(defmacro upstream [] (let [events (atom []) observe (fn* [v] (swap! events conj v) v)] (list 'quote [(try [:returned (do {original})] (catch :default e [:threw (cond (instance? js/TypeError e) \"type-error\" (instance? js/RangeError e) \"range-error\" (instance? js/ReferenceError e) \"reference-error\" :else \"error\") (.-message e)])) @events]))) (upstream)"
                    )
                } else {
                    format!("(fn* [observe] {original})")
                };
                let mut fresh = MacroHost::new();
                let syntax = light_cljs::parse(&source).unwrap();
                let program = light_cljs::analyze_with_macros(
                    &source,
                    &syntax,
                    if reuse { &mut reused } else { &mut fresh },
                )
                .unwrap();
                let javascript = light_cljs::emitter::emit_js_unit(&program);
                let invocation = if phase {
                    format!("({javascript})(rt)")
                } else {
                    format!("({javascript})(rt)(observe)")
                };
                let script = format!(
                    "(() => {{const rt={}; return ({})(rt)('{id}', observe=>{invocation});}})()",
                    light_cljs::evaluator::RUNTIME_SOURCE,
                    include_str!("support/sci_execution_observation.js")
                );
                for (engine, raw) in observe(&script) {
                    writeln!(
                        output,
                        "{id}\t{variant}\t{engine}\t{}\t{}\t{}\t{}",
                        hex(&source),
                        hex(&javascript),
                        hex(&raw),
                        if reuse { hex(&warmup) } else { "-".into() }
                    )
                    .unwrap();
                    count += 1;
                }
            }
        }
    }
    assert!(!input.is_empty());
    assert_eq!(input.lines().count() * 8, count);
    output.flush().unwrap();
}
