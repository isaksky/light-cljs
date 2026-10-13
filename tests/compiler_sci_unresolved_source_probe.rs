//! Candidate audit: all seven original unresolved SCI source contracts.
//! Frames record failures honestly; no catalog or semantic credit is written.
use light_cljs::{MacroHost, analyzer::NamespaceInput};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};
fn hex(s: &str) -> String {
    s.as_bytes().iter().map(|b| format!("{b:02x}")).collect()
}
fn unhex(s: &str) -> String {
    String::from_utf8(
        s.as_bytes()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
            .collect(),
    )
    .unwrap()
}
fn execute(script: &str) -> [(String, String); 2] {
    let rt = rquickjs::Runtime::new().unwrap();
    rt.set_memory_limit(128 * 1024 * 1024);
    rt.set_max_stack_size(4 * 1024 * 1024);
    let context = rquickjs::Context::full(&rt).unwrap();
    let quickjs = context.with(|ctx| match ctx.eval::<String, _>(script.as_bytes()) {
        Ok(s) => s,
        Err(e) => format!("HARNESS:{e:?}:{:?}", ctx.catch()),
    });
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
    let output = node.wait_with_output().unwrap();
    let node = if output.status.success() && output.stderr.is_empty() {
        String::from_utf8(output.stdout).unwrap()
    } else {
        format!(
            "HARNESS:{}:{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        )
    };
    [("QuickJS".into(), quickjs), ("Node".into(), node)]
}
#[test]
#[ignore = "Explicit independently owned seven-source evidence candidate"]
fn unchanged_seven_sources_execute_actual_phases_and_lifecycles() {
    let rows = fs::read_to_string(std::env::var("LIGHT_SCI_UNRESOLVED_INPUT").unwrap()).unwrap();
    assert_eq!(rows.lines().count(), 7);
    let root = PathBuf::from(std::env::var("LIGHT_SCI_UNRESOLVED_ROOT").unwrap());
    assert!(!root.exists());
    let mut out = fs::File::create(std::env::var("LIGHT_SCI_UNRESOLVED_OUTPUT").unwrap()).unwrap();
    let mut reused = MacroHost::new();
    let warmup = light_cljs::compile_with_macros(
        "(defmacro phase-ready [] (+ 20 22)) (phase-ready)",
        &mut reused,
    )
    .unwrap();
    for row in rows.lines() {
        let f: Vec<_> = row.split('\t').collect();
        assert_eq!(f.len(), 2);
        let (id, original) = (f[0], unhex(f[1]));
        assert!(id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-'));
        let syntax = light_cljs::parse(&original).unwrap();
        let last = syntax.nodes[syntax.roots.last().unwrap().0].span.clone();
        for (phase, reuse) in [
            ("application", false),
            ("macro", false),
            ("application", true),
            ("macro", true),
        ] {
            let variant = format!("{phase}-{}", if reuse { "reused" } else { "fresh" });
            let source = if phase == "macro" {
                format!(
                    "{}(defmacro upstream [] (list 'quote {})) (upstream)",
                    &original[..last.start],
                    &original[last.clone()]
                )
            } else {
                original.clone()
            };
            let dir = root.join(id).join(&variant);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("original.cljs"), &original).unwrap();
            fs::write(dir.join("phase.cljs"), &source).unwrap();
            let mut fresh = MacroHost::new();
            let setup = "(ns user) (def observe (fn [x] (js/globalThis.lightObserve x)))";
            let setup_syntax = light_cljs::parse(setup).unwrap();
            let mut syntax = light_cljs::parse(&source).unwrap();
            let dummy = light_cljs::parse("nil").unwrap().nodes[0].clone();
            let header = light_cljs::parser::NodeId(syntax.nodes.len());
            syntax.nodes.push(dummy);
            syntax.roots.insert(0, header);
            let inputs = [
                NamespaceInput {
                    name: "user",
                    source: setup,
                    syntax: &setup_syntax,
                    aliases: &[],
                    requires: &[],
                },
                NamespaceInput {
                    name: "user",
                    source: &source,
                    syntax: &syntax,
                    aliases: &[],
                    requires: &[],
                },
            ];
            let result = light_cljs::analyzer::analyze_namespaces_with_macros(
                &inputs,
                if reuse { &mut reused } else { &mut fresh },
            );
            match result {
                Err(e) => {
                    for engine in ["QuickJS", "Node"] {
                        writeln!(
                            out,
                            "{id}\t{variant}\t{engine}\trejected\t{}\t-\t{}\t{}",
                            hex(&source),
                            hex(&format!("{e:?}")),
                            if reuse { hex(&warmup) } else { "-".into() }
                        )
                        .unwrap();
                    }
                }
                Ok(program) => {
                    let js = light_cljs::emitter::emit_js_unit(&program);
                    let script = format!(
                        "(() => {{ const rt={}; return ({})(rt)('{id}',observe=>{{globalThis.lightObserve=observe; return ({js})(rt);}}); }})()",
                        light_cljs::evaluator::RUNTIME_SOURCE,
                        include_str!("support/sci_execution_observation.js")
                    );
                    for (engine, raw) in execute(&script) {
                        writeln!(
                            out,
                            "{id}\t{variant}\t{engine}\texecuted\t{}\t{}\t{}\t{}",
                            hex(&source),
                            hex(&js),
                            hex(&raw),
                            if reuse { hex(&warmup) } else { "-".into() }
                        )
                        .unwrap();
                    }
                }
            }
            out.flush().unwrap();
        }
    }
}
