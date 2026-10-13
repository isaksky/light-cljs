//! Original source positions and an explicit SCI user namespace, with source
//! declarations compiled before application execution in fresh Node/QuickJS heaps.
use light_cljs::analyzer::NamespaceInput;
use std::{
    fs,
    io::Write,
    process::{Command, Stdio},
};

fn unhex(text: &str) -> String {
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

fn script(id: &str, source: &str) -> Result<String, String> {
    let setup = "(ns user) (def observe (fn [x] (js/globalThis.lightObserve x)))";
    let setup_syntax = light_cljs::parse(setup).map_err(|e| format!("{e:?}"))?;
    let mut syntax = light_cljs::parse(source).map_err(|e| format!("{e:?}"))?;
    // NamespaceInput's first root is its validated header. The test fixture
    // supplies user explicitly and keeps every original source byte/span.
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
            source,
            syntax: &syntax,
            aliases: &[],
            requires: &[],
        },
    ];
    let program =
        light_cljs::analyzer::analyze_namespaces(&inputs).map_err(|e| format!("{e:?}"))?;
    let unit = light_cljs::emitter::emit_js_unit(&program);
    Ok(format!(
        "(() => {{ const rt = {}; return ({})(rt)('{id}', observe => {{ globalThis.lightObserve = observe; return ({unit})(rt); }}); }})()",
        light_cljs::evaluator::RUNTIME_SOURCE,
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

#[test]
fn original_source_macro_declarations_execute_phase_and_application_contracts() {
    for (id, source, expected) in [
        (
            "raw",
            "(defmacro m [x] (list 'quote x)) (m unresolved)",
            r#"{"id":"raw","outcome":{"status":"returned","value":{"$symbol":{"namespace":null,"name":"unresolved","spelling":"unresolved"}}},"trace":[],"stdout":""}"#,
        ),
        (
            "twice",
            "(defmacro twice [x] (list 'do x x)) (twice (observe 4))",
            r#"{"id":"twice","outcome":{"status":"returned","value":4},"trace":[4,4],"stdout":""}"#,
        ),
        (
            "redefinition",
            "(defmacro m [] 1) (def f (fn* [] (m))) (defmacro m [] 2) [(f) (m)]",
            r#"{"id":"redefinition","outcome":{"status":"returned","value":{"$vector":[1,2]}},"trace":[],"stdout":""}"#,
        ),
        (
            "phase",
            "(def ^:dynamic x 1) (defmacro m [] x) [(binding [x 2] (m)) x]",
            r#"{"id":"phase","outcome":{"status":"returned","value":{"$vector":[1,1]}},"trace":[],"stdout":""}"#,
        ),
        (
            "location",
            "(defn f [x] x) [(get (meta #'f) :line) (get (meta #'f) :column)]",
            r#"{"id":"location","outcome":{"status":"returned","value":{"$vector":[1,1]}},"trace":[],"stdout":""}"#,
        ),
    ] {
        let script = script(id, source).unwrap();
        let (node, quickjs) = execute(&script);
        assert_eq!(node, expected, "{id}: Node");
        assert_eq!(quickjs, expected, "{id}: QuickJS");
    }
}

#[test]
#[ignore = "explicit original SCI declaration/namespace context audit"]
fn retained_sources_execute_with_original_namespace_and_positions() {
    let input = std::env::var("LIGHT_CLJS_SCI_CONTEXT_INPUT").expect("source TSV");
    let output = std::env::var("LIGHT_CLJS_SCI_CONTEXT_OUTPUT").expect("observation TSV");
    let mut out = fs::File::create(output).unwrap();
    let mut count = 0;
    for line in fs::read_to_string(input).unwrap().lines() {
        let fields: Vec<_> = line.split('\t').collect();
        assert_eq!(fields.len(), 2);
        assert!(
            fields[0]
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-')
        );
        let source = unhex(fields[1]);
        match script(fields[0], &source) {
            Ok(script) => {
                let (node, quickjs) = execute(&script);
                writeln!(out, "{}\texecuted\t{node}\t{quickjs}", fields[0]).unwrap();
            }
            Err(error) => writeln!(
                out,
                "{}\trejected\t{}",
                fields[0],
                error.replace(['\n', '\t'], " ")
            )
            .unwrap(),
        }
        count += 1;
    }
    assert!(count > 0, "zero source executions");
}
