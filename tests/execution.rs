use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

use light_cljs::compile;

#[path = "support/generated.rs"]
mod generated;

#[path = "support/language_combinations.rs"]
mod language_combinations;

#[derive(Clone, Debug)]
struct Case {
    id: String,
    source: String,
    outcome: String,
    trace: String,
    stdout: String,
}

fn rows(text: &'static str) -> impl Iterator<Item = Vec<&'static str>> {
    text.lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .map(|line| line.split('\t').collect())
}

fn cases() -> Vec<Case> {
    rows(include_str!("fixtures/execution.tsv"))
        .map(|row| {
            let [id, source, value, trace]: [&str; 4] = row.try_into().unwrap();
            Case {
                id: id.into(),
                source: source.into(),
                outcome: format!("{{\"status\":\"returned\",\"value\":{value}}}"),
                trace: trace.into(),
                stdout: "\"\"".into(),
            }
        })
        .chain(
            rows(include_str!("fixtures/execution-outcomes.tsv")).map(|row| {
                let [id, source, outcome, trace, stdout]: [&str; 5] = row.try_into().unwrap();
                Case {
                    id: id.into(),
                    source: source.into(),
                    outcome: outcome.into(),
                    trace: trace.into(),
                    stdout: stdout.into(),
                }
            }),
        )
        .chain(
            rows(include_str!("fixtures/execution-literals.tsv")).map(|row| {
                let [id, hex, value, trace]: [&str; 4] = row.try_into().unwrap();
                let (pairs, remainder) = hex.as_bytes().as_chunks::<2>();
                assert!(remainder.is_empty(), "incomplete source byte for {id}");
                let source = String::from_utf8(
                    pairs
                        .iter()
                        .map(|pair| {
                            u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap()
                        })
                        .collect(),
                )
                .unwrap();
                Case {
                    id: id.into(),
                    source,
                    outcome: format!("{{\"status\":\"returned\",\"value\":{value}}}"),
                    trace: trace.into(),
                    stdout: "\"\"".into(),
                }
            }),
        )
        .collect()
}

// Enough JSON for fixture metadata and exact expected protocol lines, without
// relying on the encoder under test to manufacture its own expected values.
fn json_string(value: &str) -> String {
    let mut result = String::from("\"");
    for c in value.chars() {
        match c {
            '"' => result.push_str("\\\""),
            '\\' => result.push_str("\\\\"),
            c if c.is_control() => result.push_str(&format!("\\u{:04x}", c as u32)),
            c => result.push(c),
        }
    }
    result.push('"');
    result
}

struct RunFailure {
    message: String,
    _dir: TempDir,
}

fn try_run(mut command: Command, input: &[u8], timeout: Duration) -> Result<Output, RunFailure> {
    let dir = TempDir::new("process");
    let description = format!("{command:?}");
    fs::write(dir.0.join("invocation.txt"), &description).unwrap();
    fs::write(dir.0.join("input.js"), input).unwrap();
    // File-backed streams retain partial progress on a crash/hang, with no pipe
    // reader/writer joins that could outlive the process timeout.
    let child = command
        .stdin(fs::File::open(dir.0.join("input.js")).unwrap())
        .stdout(fs::File::create(dir.0.join("stdout.txt")).unwrap())
        .stderr(fs::File::create(dir.0.join("stderr.txt")).unwrap())
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(error) => {
            return Err(RunFailure {
                message: format!("required tool unavailable: {description}: {error}"),
                _dir: dir,
            });
        }
    };
    let start = Instant::now();
    let mut timed_out = false;
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if start.elapsed() >= timeout {
            timed_out = true;
            child.kill().unwrap();
            break child.wait().unwrap();
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let output = Output {
        status,
        stdout: fs::read(dir.0.join("stdout.txt")).unwrap(),
        stderr: fs::read(dir.0.join("stderr.txt")).unwrap(),
    };
    if timed_out || !status.success() {
        let reason = if timed_out {
            format!("timed out after {timeout:?}")
        } else {
            format!("failed ({status})")
        };
        return Err(RunFailure {
            message: format!(
                "required tool {reason}: {description}\nartifacts: {}\n{}\n{}",
                dir.0.display(),
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ),
            _dir: dir,
        });
    }
    Ok(output)
}

fn run(command: Command, input: Vec<u8>, timeout: Duration) -> Output {
    try_run(command, &input, timeout).unwrap_or_else(|error| panic!("{}", error.message))
}

fn node_command() -> Command {
    let mut command = Command::new(std::env::var_os("NODE").unwrap_or_else(|| "node".into()));
    command.arg("-");
    command
}

fn node(script: &str) -> String {
    let output = run(
        node_command(),
        script.as_bytes().to_vec(),
        Duration::from_secs(30),
    );
    String::from_utf8(output.stdout).unwrap()
}

fn compiled_results() -> String {
    compiled_cases(&cases())
}

fn compiled_cases(cases: &[Case]) -> String {
    let mut script = String::from(
        "globalThis.lightRuntimeRegistry = [];\n\
         globalThis.lightInspectValue = value => {\n\
           for (const runtime of lightRuntimeRegistry) {\n\
             const result = runtime.inspect(value);\n\
             if (result !== null) return result;\n\
           }\n\
           return null;\n\
         };\n\
         globalThis.lightCases = [\n",
    );
    for (index, case) in cases.iter().enumerate() {
        // Match the reference namespace exactly, retaining the original source
        // separately as metadata. Aliased core is virtual in both compilers.
        let namespace = format!("semantic.case{index}");
        let source = format!(
            "(ns {namespace} (:require [cljs.core :as c]))\n(fn* [observe] {})",
            case.source
        );
        let syntax = light_cljs::parse(&source).unwrap();
        let program =
            light_cljs::analyzer::analyze_namespaces(&[light_cljs::analyzer::NamespaceInput {
                name: &namespace,
                source: &source,
                syntax: &syntax,
                aliases: &[("c".into(), "cljs.core".into())],
                requires: &["cljs.core".into()],
            }])
            .unwrap_or_else(|(_, error)| panic!("{}: {}: {error}", case.id, case.source));
        let js = light_cljs::emit_js(&program);
        // Test-only instrumentation captures the lexical factory instance. The
        // production prelude has no global observer/brand exposure.
        script.push_str(&js.replacen(
            " = // Original bounded runtime",
            " = globalThis.lightRuntimeRegistry[globalThis.lightRuntimeRegistry.length] = // Original bounded runtime",
            1,
        ));
        script.push_str(",\n");
    }
    script.push_str("];\nglobalThis.lightCaseMetadata = [\n");
    for case in cases {
        script.push_str(&format!(
            "[{},{}],\n",
            json_string(&case.id),
            json_string(&case.source)
        ));
    }
    script.push_str("];\n");
    script.push_str(include_str!("support/execution.js"));
    node(&script)
}

fn outcomes<'a>(output: &'a str, cases: &[Case]) -> Result<Vec<&'a str>, String> {
    let lines: Vec<_> = output.lines().collect();
    for (index, case) in cases.iter().enumerate() {
        let prefix = format!("{{\"id\":{},\"outcome\":", json_string(&case.id));
        if !lines
            .get(index)
            .is_some_and(|line| line.starts_with(&prefix))
        {
            return Err(format!(
                "missing, duplicate, or out-of-order outcome for {}: {} at index {index}; got {:?}",
                case.id,
                case.source,
                lines.get(index)
            ));
        }
    }
    if lines.len() != cases.len() {
        return Err(format!(
            "expected {} outcomes, got {}: {output}",
            cases.len(),
            lines.len()
        ));
    }
    Ok(lines)
}

#[test]
fn tiny_programs_execute_with_expected_values_and_effects() {
    let output = compiled_results();
    let cases = cases();
    for (case, actual) in cases.iter().zip(outcomes(&output, &cases).unwrap()) {
        assert_eq!(
            actual,
            format!(
                "{{\"id\":{},\"outcome\":{},\"trace\":{},\"stdout\":{}}}",
                json_string(&case.id),
                case.outcome,
                case.trace,
                case.stdout
            ),
            "{}: {}",
            case.id,
            case.source
        );
    }
}

fn harness_script(programs: &[(&str, &str, &str)]) -> String {
    let functions = programs
        .iter()
        .map(|(_, _, function)| *function)
        .collect::<Vec<_>>()
        .join(",");
    let metadata = programs
        .iter()
        .map(|(id, source, _)| format!("[{},{}]", json_string(id), json_string(source)))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "globalThis.lightCases = [{functions}];\nglobalThis.lightCaseMetadata = [{metadata}];\n{}",
        include_str!("support/execution.js")
    )
}

#[test]
fn harness_encodes_special_values_and_continues_after_failures_independently() {
    // JavaScript probes intentionally bypass both compilers. Literal expected
    // bytes catch shared serializer/runner mistakes that differential tests miss.
    let script = harness_script(&[
        (
            "special",
            "host-only special-value probe",
            r#"observe => {
            const value = [null, undefined, -0, NaN, Infinity, -Infinity];
            observe(value);
            observe({missing: undefined, nested: [NaN], literalTag: {$number: "NaN"}});
            return value;
        }"#,
        ),
        (
            "throw",
            "host-only thrown-value probe",
            r#"observe => {
            observe(undefined);
            lightHost.value = 99;
            console.log("before");
            process.stdout.write(Buffer.from([0xc3]));
            process.stdout.write(Buffer.from([0xa9]));
            throw undefined;
        }"#,
        ),
        (
            "after",
            "host-only continuation probe",
            r#"observe => {
            console.log("after");
            observe(lightHost.value);
            return null;
        }"#,
        ),
    ]);
    assert_eq!(
        node(&script),
        concat!(
            r#"{"id":"special","outcome":{"status":"returned","value":[null,{"$value":"undefined"},{"$number":"-0"},{"$number":"NaN"},{"$number":"+Infinity"},{"$number":"-Infinity"}]},"trace":[[null,{"$value":"undefined"},{"$number":"-0"},{"$number":"NaN"},{"$number":"+Infinity"},{"$number":"-Infinity"}],{"$object":{"missing":{"$value":"undefined"},"nested":[{"$number":"NaN"}],"literalTag":{"$object":{"$number":"NaN"}}}}],"stdout":""}"#,
            "\n",
            r#"{"id":"throw","outcome":{"status":"threw","error":{"category":"thrown-value","value":{"$value":"undefined"}}},"trace":[{"$value":"undefined"}],"stdout":"before\né"}"#,
            "\n",
            r#"{"id":"after","outcome":{"status":"returned","value":null},"trace":[40],"stdout":"after\n"}"#,
            "\n",
        )
    );
}

#[test]
fn harness_preserves_utf16_units_and_embedded_controls_independently() {
    // Never pass a lone surrogate through a UTF-8 conversion. JSON escapes it,
    // while a valid pair may use its scalar UTF-8 spelling without losing units.
    let script = harness_script(&[(
        "utf16",
        "host-only UTF-16 and control probe",
        r#"observe => {
            const s = "\ud800A\udc00\ud83d\ude00\u0000\r\n\t\b\f";
            observe(Array.from({length: s.length}, (_, i) => s.charCodeAt(i)));
            return s;
        }"#,
    )]);
    assert_eq!(
        node(&script),
        concat!(
            r#"{"id":"utf16","outcome":{"status":"returned","value":"\ud800A\udc00😀\u0000\r\n\t\b\f"},"trace":[[55296,65,56320,55357,56832,0,13,10,9,8,12]],"stdout":""}"#,
            "\n",
        )
    );
}

#[test]
fn harness_distinguishes_trusted_language_values_and_forged_host_tags() {
    // Private WeakMap membership models a trusted backend adapter without
    // relying on either implementation to compute these expected bytes.
    let setup = r#"
        const branded = new WeakMap();
        const wrap = data => { const value = Object.freeze({}); branded.set(value, data); return value; };
        globalThis.lightInspectValue = value => branded.get(value) || null;
        const keyword = (namespace, name, spelling = namespace === null ? name : namespace + "/" + name) => wrap({kind: "keyword", namespace, name, spelling});
        const vector = (...items) => wrap({kind: "vector", items});
    "#;
    let script = String::from(setup)
        + &harness_script(&[(
            "typed",
            "independent typed, forged, repeated and UTF-16 values",
            r#"observe => {
                const k = keyword("n\ud800", "x\udc00");
                const v = vector(k, "x\udc00", vector(), null, false, k);
                observe(v);
                observe({$keyword: {namespace: "n\ud800", name: "x\udc00"}, $vector: [k]});
                observe({kind: "keyword", namespace: "n\ud800", name: "x\udc00", spelling: "n\ud800/x\udc00"});
                return [v, vector(v), k];
            }"#,
        )]);
    assert_eq!(
        node(&script),
        concat!(
            r#"{"id":"typed","outcome":{"status":"returned","value":[{"$vector":[{"$keyword":{"namespace":"n\ud800","name":"x\udc00","spelling":"n\ud800/x\udc00"}},"x\udc00",{"$vector":[]},null,false,{"$keyword":{"namespace":"n\ud800","name":"x\udc00","spelling":"n\ud800/x\udc00"}}]},{"$vector":[{"$vector":[{"$keyword":{"namespace":"n\ud800","name":"x\udc00","spelling":"n\ud800/x\udc00"}},"x\udc00",{"$vector":[]},null,false,{"$keyword":{"namespace":"n\ud800","name":"x\udc00","spelling":"n\ud800/x\udc00"}}]}]},{"$keyword":{"namespace":"n\ud800","name":"x\udc00","spelling":"n\ud800/x\udc00"}}]},"trace":[{"$vector":[{"$keyword":{"namespace":"n\ud800","name":"x\udc00","spelling":"n\ud800/x\udc00"}},"x\udc00",{"$vector":[]},null,false,{"$keyword":{"namespace":"n\ud800","name":"x\udc00","spelling":"n\ud800/x\udc00"}}]},{"$object":{"$keyword":{"$object":{"namespace":"n\ud800","name":"x\udc00"}},"$vector":[{"$keyword":{"namespace":"n\ud800","name":"x\udc00","spelling":"n\ud800/x\udc00"}}]}},{"$object":{"kind":"keyword","namespace":"n\ud800","name":"x\udc00","spelling":"n\ud800/x\udc00"}}],"stdout":""}"#,
            "\n",
        )
    );
    let identity = String::from(setup)
        + &harness_script(&[(
            "identity",
            "same name and namespace retain distinct full keyword spellings",
            "() => [keyword(null, 'a', 'a/b/c'), keyword(null, 'a', 'a'), keyword(undefined, 'x', 'x')]",
        )]);
    assert_eq!(
        node(&identity),
        concat!(
            r#"{"id":"identity","outcome":{"status":"returned","value":[{"$keyword":{"namespace":null,"name":"a","spelling":"a/b/c"}},{"$keyword":{"namespace":null,"name":"a","spelling":"a"}},{"$keyword":{"namespace":{"$value":"undefined"},"name":"x","spelling":"x"}}]},"trace":[],"stdout":""}"#,
            "\n",
        )
    );
}

#[test]
fn harness_rejects_cycles_through_language_values_and_invalid_adapters() {
    for adapter in [
        "value => ({kind: 'vector', items: [value]})",
        "value => value.root === true ? ({kind: 'vector', items: [{back: value}]}) : null",
        "value => ({kind: 'keyword', namespace: 1, name: 'x'})",
        "value => ({kind: 'vector', items: null})",
        "value => ({kind: 'invented'})",
    ] {
        let script = format!("globalThis.lightInspectValue = {adapter};\n")
            + &harness_script(&[("adapter", "invalid typed transport", "() => ({root: true})")]);
        let error = try_run(node_command(), script.as_bytes(), Duration::from_secs(5))
            .expect_err("invalid adapters and cyclic values must fail outside program catch");
        assert!(error.message.contains("required tool failed"));
        assert!(
            fs::read(error._dir.0.join("stdout.txt"))
                .unwrap()
                .is_empty()
        );
    }
}

#[test]
fn runtime_values_have_private_immutable_storage() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/support/js_runtime.test.js");
    node(&format!(
        "require({});",
        json_string(path.to_str().unwrap())
    ));
}

#[test]
fn harness_rejects_invalid_cases_and_unserializable_values() {
    for script in [
        format!(
            "globalThis.lightCases = []; globalThis.lightCaseMetadata = []; {}",
            include_str!("support/execution.js")
        ),
        format!(
            "globalThis.lightCases = [() => 1]; globalThis.lightCaseMetadata = []; {}",
            include_str!("support/execution.js")
        ),
        format!(
            "globalThis.lightCases = Array(1); globalThis.lightCaseMetadata = [['missing', 'missing executable']]; {}",
            include_str!("support/execution.js")
        ),
        format!(
            "globalThis.lightCases = [() => 1]; globalThis.lightCaseMetadata = [Array(2)]; {}",
            include_str!("support/execution.js")
        ),
        harness_script(&[
            ("duplicate", "one", "() => 1"),
            ("duplicate", "two", "() => 2"),
        ]),
        harness_script(&[("function", "unsupported host function", "() => () => 1")]),
        harness_script(&[(
            "cycle",
            "cyclic host array",
            "() => { const a = []; a.push(a); return a; }",
        )]),
        harness_script(&[(
            "trace",
            "unsupported trace value",
            "observe => { observe(Symbol()); return 1; }",
        )]),
    ] {
        let error = try_run(node_command(), script.as_bytes(), Duration::from_secs(5))
            .expect_err("invalid harness data must fail");
        assert!(
            error.message.contains("required tool failed"),
            "{}",
            error.message
        );
        assert!(
            fs::read(error._dir.0.join("stdout.txt"))
                .unwrap()
                .is_empty(),
            "harness failure must not become a program outcome"
        );
    }
}

#[test]
fn process_failures_and_timeouts_retain_current_case_identity_and_source() {
    for (id, source, program, timeout, reason) in [
        (
            "crash-probe",
            "(js/process.exit 7)",
            "() => { process.exit(7); }",
            Duration::from_secs(5),
            "failed",
        ),
        (
            "hang-probe",
            "host-only infinite loop",
            "() => { while (true) {} }",
            Duration::from_secs(2),
            "timed out",
        ),
    ] {
        let script = harness_script(&[(id, source, program)]);
        let started = Instant::now();
        let error = try_run(node_command(), script.as_bytes(), timeout)
            .expect_err("process failure must not succeed");
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "process timeout must remain bounded"
        );
        for expected in [reason, "CASE START", id, source] {
            assert!(
                error.message.contains(expected),
                "missing {expected}: {}",
                error.message
            );
        }
        assert_eq!(
            fs::read_to_string(error._dir.0.join("input.js")).unwrap(),
            script
        );
    }
}

#[test]
fn missing_duplicate_reordered_and_extra_outcomes_fail() {
    let cases: Vec<_> = cases().into_iter().take(2).collect();
    let nil = r#"{"id":"nil","outcome":{"status":"returned","value":null},"trace":[],"stdout":""}"#;
    let yes =
        r#"{"id":"true","outcome":{"status":"returned","value":true},"trace":[],"stdout":""}"#;
    assert!(outcomes(&format!("{nil}\n{yes}\n"), &cases).is_ok());
    for output in [
        String::new(),
        format!("{nil}\n"),
        format!("{nil}\n{nil}\n"),
        format!("{yes}\n{nil}\n"),
        format!("{nil}\n{yes}\n{yes}\n"),
    ] {
        assert!(
            outcomes(&output, &cases).is_err(),
            "accepted incomplete/ambiguous output: {output}"
        );
    }
    let script = harness_script(&[(
        "silent-exit",
        "(js/process.exit 0)",
        "() => { process.exit(0); }",
    )]);
    let output = node(&script);
    let case = Case {
        id: "silent-exit".into(),
        source: "(js/process.exit 0)".into(),
        outcome: String::new(),
        trace: "[]".into(),
        stdout: "\"\"".into(),
    };
    let error = outcomes(&output, &[case]).unwrap_err();
    assert!(
        error.contains("silent-exit") && error.contains("(js/process.exit 0)"),
        "{error}"
    );
}

#[test]
fn empty_files_and_top_level_forms_execute() {
    for (source, expected) in [
        ("", "null"),
        ("; empty\n#_1", "null"),
        ("1 2 3", "3"),
        ("(let* [x 10 f (fn* [] x)] (let* [x 20] (f)))", "10"),
    ] {
        let js = compile(source).unwrap();
        assert_eq!(
            node(&format!("console.log(JSON.stringify({js}));")),
            format!("{expected}\n")
        );
    }
}

#[test]
fn deeply_nested_accepted_source_compiles_and_executes() {
    let source = format!("{}0{}", "(let* [] ".repeat(127), ")".repeat(127));
    let js = compile(&source).unwrap();
    assert_eq!(node(&format!("console.log({js});")), "0\n");

    for (opening, closing) in [("(identity ", ")"), ("(if true ", " nil)")] {
        let source = format!("{}0{}", opening.repeat(127), closing.repeat(127));
        let js = compile(&source).unwrap();
        assert_eq!(
            node(&format!("console.log({js});")),
            "0\n",
            "nested {opening} forms"
        );
    }
}

#[test]
fn large_vector_literals_do_not_use_the_javascript_argument_stack() {
    // Well beyond Node's argument-count limit: literal storage is constructed
    // from an array, while ordinary variadic core calls retain their call ABI.
    let source = format!("(count [{}])", "0 ".repeat(140_000));
    let js = compile(&source).unwrap();
    assert_eq!(node(&format!("console.log({js});")), "140000\n");
}

struct TempDir(PathBuf);
impl TempDir {
    fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let path = std::env::temp_dir().join(format!(
                "light-cljs-{label}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!(
                    "cannot create execution artifacts at {}: {error}",
                    path.display()
                ),
            }
        }
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!(
                "Execution failure artifacts retained at {}",
                self.0.display()
            );
        } else {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}

#[derive(Debug, PartialEq)]
enum Reference {
    Release,
    Checkout { path: PathBuf, revision: String },
}

fn references(
    selection: &str,
    checkout: Option<PathBuf>,
    revision: Option<String>,
) -> Result<Vec<Reference>, String> {
    match selection {
        "release" => Ok(vec![Reference::Release]),
        "checkout" | "both" => {
            let path =
                checkout.ok_or("checkout comparison requires LIGHT_CLJS_REFERENCE_CHECKOUT")?;
            let revision = revision.ok_or(
                "checkout comparison requires LIGHT_CLJS_REFERENCE_REVISION (full commit SHA)",
            )?;
            if revision.len() != 40
                || !revision
                    .bytes()
                    .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
            {
                return Err(
                    "LIGHT_CLJS_REFERENCE_REVISION must be a full lowercase commit SHA".into(),
                );
            }
            let checkout = Reference::Checkout { path, revision };
            Ok(if selection == "both" {
                vec![Reference::Release, checkout]
            } else {
                vec![checkout]
            })
        }
        _ => Err("LIGHT_CLJS_REFERENCE must be release, checkout, or both".into()),
    }
}

#[test]
fn reference_selection_requires_explicit_checkout_identity() {
    assert_eq!(
        references("release", None, None).unwrap(),
        [Reference::Release]
    );
    assert!(references("unknown", None, None).is_err());
    assert!(references("checkout", None, None).is_err());
    assert!(references("both", Some("research/clojurescript".into()), None).is_err());
    assert!(references("checkout", Some("source".into()), Some("HEAD".into())).is_err());
    let revision = "a".repeat(40);
    assert_eq!(
        references("both", Some("source".into()), Some(revision.clone())).unwrap(),
        [
            Reference::Release,
            Reference::Checkout {
                path: "source".into(),
                revision
            }
        ]
    );
}

fn shell_quote(value: &std::ffi::OsStr) -> String {
    format!("'{}'", value.to_string_lossy().replace('\'', "'\\''"))
}

// Write directly to files: failures/timeouts retain diagnostics even when a
// launcher leaves descendants holding stdout/stderr open.
fn run_reference(mut command: Command, dir: &Path, timeout: Duration) -> String {
    let invocation = format!(
        "cd {} && {} {}\n",
        shell_quote(command.get_current_dir().unwrap().as_os_str()),
        shell_quote(command.get_program()),
        command
            .get_args()
            .map(shell_quote)
            .collect::<Vec<_>>()
            .join(" ")
    );
    fs::write(dir.join("invocation.sh"), &invocation).unwrap();
    eprintln!("REFERENCE INVOCATION {}", invocation.trim());
    let mut child = command
        .stdin(Stdio::null())
        .stdout(fs::File::create(dir.join("build.stdout")).unwrap())
        .stderr(fs::File::create(dir.join("build.stderr")).unwrap())
        .spawn()
        .unwrap_or_else(|error| panic!("required tool unavailable: {invocation}: {error}"));
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if start.elapsed() >= timeout {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("required tool timed out after {timeout:?}: {invocation}");
        }
        std::thread::sleep(Duration::from_millis(25));
    };
    let stdout = fs::read_to_string(dir.join("build.stdout")).unwrap();
    let stderr = fs::read_to_string(dir.join("build.stderr")).unwrap();
    assert!(
        status.success(),
        "required tool failed: {invocation}\n{stdout}\n{stderr}"
    );
    if !stderr.is_empty() {
        eprintln!("{stderr}");
    }
    stdout
}

fn reference_build(
    reference: &Reference,
    label: &str,
    args: &[&str],
    macro_roots: &[PathBuf],
) -> TempDir {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let dir = TempDir::new(&format!("reference-{label}"));
    let (kind, identity, checkout, dependency) = match reference {
        Reference::Release => (
            "release",
            "1.12.134",
            "-".to_owned(),
            "{:mvn/version \"1.12.134\"}".to_owned(),
        ),
        Reference::Checkout { path, revision } => {
            let path = fs::canonicalize(path).unwrap_or_else(|error| {
                panic!(
                    "required checkout unavailable at {}: {error}",
                    path.display()
                )
            });
            let path = path
                .to_str()
                .expect("checkout path must be UTF-8")
                .to_owned();
            assert!(
                !path.chars().any(char::is_control),
                "checkout path contains control characters"
            );
            let edn_path = path.replace('\\', "\\\\").replace('"', "\\\"");
            (
                "checkout",
                revision.as_str(),
                path,
                format!("{{:local/root \"{edn_path}\"}}"),
            )
        }
    };
    // Explicit JVM classpath roots are needed for .clj macro imports. Build
    // inputs alone discover .cljs files but do not make their macros loadable.
    let paths = macro_roots
        .iter()
        .map(|path| {
            let path = fs::canonicalize(path).expect("fixture macro source root must exist");
            let path = path.to_str().expect("macro source root must be UTF-8");
            assert!(
                !path.chars().any(char::is_control),
                "macro root contains controls"
            );
            json_string(path)
        })
        .collect::<Vec<_>>()
        .join(" ");
    let mut command = Command::new(std::env::var_os("CLOJURE").unwrap_or_else(|| "clojure".into()));
    command
        .current_dir(&root)
        .args([
            "-Srepro",
            "-Sdeps",
            &format!("{{:paths [{paths}] :deps {{org.clojure/clojurescript {dependency}}}}}"),
            "-M",
            "tests/reference/execution.clj",
            kind,
            identity,
            &checkout,
        ])
        .arg(&dir.0)
        .args(args);
    let timeout = std::env::var("LIGHT_CLJS_REFERENCE_TIMEOUT_SECONDS")
        .map(|value| {
            value
                .parse::<u64>()
                .expect("reference timeout must be seconds")
        })
        .unwrap_or(180);
    assert!(timeout > 0, "reference timeout must be positive");
    let settings = run_reference(command, &dir.0, Duration::from_secs(timeout));
    assert!(
        settings.contains(&format!("REFERENCE IDENTITY {kind} {identity}")),
        "missing verified reference identity: {settings}"
    );
    assert!(
        settings.contains(
            "REFERENCE SETTINGS {:optimizations :none, :static-fns false, :def-emits-var false}"
        ),
        "missing verified reference settings: {settings}"
    );
    assert!(
        dir.0
            .join(if args.first() == Some(&"analysis") {
                "analysis.tsv"
            } else {
                "reference.js"
            })
            .is_file(),
        "reference compiler produced no results"
    );
    eprintln!("{}", settings.trim());
    dir
}

#[test]
#[ignore = "requires Clojure CLI, Java, Node, and the pinned ClojureScript Maven dependencies"]
fn analyzer_boundary_classifications_match_reference() {
    let selection = match std::env::var("LIGHT_CLJS_REFERENCE") {
        Ok(value) => value,
        Err(std::env::VarError::NotPresent) => "release".into(),
        Err(error) => panic!("invalid LIGHT_CLJS_REFERENCE: {error}"),
    };
    let selected = references(
        &selection,
        std::env::var_os("LIGHT_CLJS_REFERENCE_CHECKOUT").map(PathBuf::from),
        std::env::var("LIGHT_CLJS_REFERENCE_REVISION").ok(),
    )
    .unwrap();
    let mut outputs = Vec::new();
    let mut artifacts = Vec::new();
    for reference in selected {
        let dir = reference_build(
            &reference,
            "analysis",
            &["analysis", "tests/fixtures/analysis-boundaries.tsv"],
            &[],
        );
        let output = fs::read_to_string(dir.0.join("analysis.tsv")).unwrap();
        let actual: Vec<_> = output.lines().collect();
        let expected: Vec<_> = rows(include_str!("fixtures/analysis-boundaries.tsv")).collect();
        assert_eq!(actual.len(), expected.len(), "missing boundary outcomes");
        for (actual, expected) in actual.iter().zip(&expected) {
            let columns: Vec<_> = actual.split('\t').collect();
            assert_eq!(columns.len(), 4, "malformed boundary result: {actual}");
            assert_eq!(
                columns[0], expected[0],
                "missing/reordered boundary outcome"
            );
            let expected_column = if reference == Reference::Release {
                7
            } else {
                8
            };
            assert_eq!(
                columns[1], expected[expected_column],
                "{reference:?}: {actual}"
            );
            if columns[1] == "warning" {
                assert!(
                    !columns[2].is_empty(),
                    "warning category needs warning evidence"
                );
            }
        }
        eprintln!(
            "Matched {} analyzer boundary classifications against {reference:?}",
            actual.len()
        );
        outputs.push(output);
        artifacts.push(dir);
    }
    if outputs.len() == 2 {
        for ((release, checkout), expected) in outputs[0]
            .lines()
            .zip(outputs[1].lines())
            .zip(rows(include_str!("fixtures/analysis-boundaries.tsv")))
        {
            let release: Vec<_> = release.split('\t').collect();
            let checkout: Vec<_> = checkout.split('\t').collect();
            if expected[7] == expected[8] {
                assert_eq!(
                    &release[..3],
                    &checkout[..3],
                    "UPSTREAM DISCREPANCY: unexpected boundary outcomes/warnings"
                );
            } else {
                eprintln!(
                    "Documented upstream difference for {}: release {}, checkout {}",
                    expected[0], expected[7], expected[8]
                );
            }
        }
    }
}

fn reference_driver(dir: &TempDir) -> String {
    // The reference Node launcher resolves its asset path from the process cwd.
    format!(
        "process.chdir({:?});\nrequire({:?});\n",
        dir.0.to_str().unwrap(),
        dir.0.join("reference.js").to_str().unwrap(),
    )
}

struct ReferenceResult {
    identity: String,
    output: String,
    // Keep builds until all comparisons pass; a mismatch preserves both oracles.
    _dir: TempDir,
}

fn reference_results(label: &str, args: &[&str], driver: &str) -> Vec<ReferenceResult> {
    // Existing program builds scan this entire fixture tree, including the
    // nested collection example whose .clj source needs its own classpath root.
    let macro_roots = if args.first() == Some(&"program") {
        vec![
            collection_program_root(),
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/namespaces/shared"),
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/namespaces/functions"),
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/namespaces/definitions"),
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/namespaces/integrated"),
        ]
    } else {
        Vec::new()
    };
    reference_results_with_macro_roots(label, args, driver, &macro_roots)
}

fn reference_results_with_macro_roots(
    label: &str,
    args: &[&str],
    driver: &str,
    macro_roots: &[PathBuf],
) -> Vec<ReferenceResult> {
    reference_results_policy(label, args, driver, macro_roots, true)
}

fn reference_results_policy(
    label: &str,
    args: &[&str],
    driver: &str,
    macro_roots: &[PathBuf],
    require_agreement: bool,
) -> Vec<ReferenceResult> {
    let selection = match std::env::var("LIGHT_CLJS_REFERENCE") {
        Ok(value) => value,
        Err(std::env::VarError::NotPresent) => "release".into(),
        Err(error) => panic!("invalid LIGHT_CLJS_REFERENCE: {error}"),
    };
    let references = references(
        &selection,
        std::env::var_os("LIGHT_CLJS_REFERENCE_CHECKOUT").map(PathBuf::from),
        std::env::var("LIGHT_CLJS_REFERENCE_REVISION").ok(),
    )
    .unwrap_or_else(|error| panic!("{error}"));
    let mut node_version = Command::new(std::env::var_os("NODE").unwrap_or_else(|| "node".into()));
    node_version.arg("--version");
    let node_version =
        String::from_utf8(run(node_version, Vec::new(), Duration::from_secs(30)).stdout).unwrap();
    eprintln!("REFERENCE NODE {}", node_version.trim());
    let results: Vec<_> = references
        .iter()
        .map(|reference| {
            let dir = reference_build(reference, label, args, macro_roots);
            fs::write(dir.0.join("node-version.txt"), &node_version).unwrap();
            // Observed programs initialize inside the case so namespace stdout
            // is captured, just like run/report effects. One case per process.
            let observed_program = args.first() == Some(&"program") && args.len() == 4;
            let loader = reference_driver(&dir);
            let script = if observed_program {
                format!("const loadProgram = () => {{ {loader} }};\n{driver}")
            } else if label == "cases" {
                format!("{loader}globalThis.lightExecutionDriver = {};\nglobalThis.lightIsolatedCasePrefixes = [\"extension-\", \"core-protocol-\"];\n{}", json_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/support/execution.js")), include_str!("support/isolated_execution.js"))
            } else {
                loader + driver
            };
            fs::write(dir.0.join("driver.js"), &script).unwrap();
            let output = node(&script);
            fs::write(dir.0.join("results.txt"), &output).unwrap();
            if observed_program {
                let fresh_output = node(&script);
                fs::write(dir.0.join("fresh-results.txt"), &fresh_output).unwrap();
                assert_eq!(output, fresh_output, "fresh program processes must agree");
            }
            if label == "cases" {
                outcomes(&output, &cases())
                    .unwrap_or_else(|error| panic!("{reference:?}: {error}"));
            }
            ReferenceResult {
                identity: format!("{reference:?}"),
                output,
                _dir: dir,
            }
        })
        .collect();
    if require_agreement && results.len() == 2 {
        let normalized = |result: &ReferenceResult| {
            let mut output = result.output.clone();
            for case in cases() {
                let expected = reference_case_envelope(&case, &result.identity);
                let local = case_envelope(&case, &case.outcome, &case.trace);
                output = output.replace(&expected, &local);
            }
            output
        };
        assert_eq!(
            normalized(&results[0]),
            normalized(&results[1]),
            "UPSTREAM DISCREPANCY: release versus checkout for {label}; both results retained independently of light-cljs"
        );
        eprintln!(
            "Release and checkout agree outside explicitly asserted version differences for {label}"
        );
    }
    results
}

#[test]
#[ignore = "requires Clojure CLI, Java, Node, and the pinned ClojureScript Maven dependencies"]
fn programs_match_standard_clojurescript_results_and_effects() {
    let references = reference_results("cases", &[], include_str!("support/execution.js"));
    let actual = compiled_results();
    for result in &references {
        let reference = &result.output;
        let cases = cases();
        let actual = outcomes(&actual, &cases).unwrap();
        let expected = outcomes(reference, &cases).unwrap();
        for ((case, actual), reference) in cases.iter().zip(actual).zip(expected) {
            assert_eq!(
                actual,
                case_envelope(case, &case.outcome, &case.trace),
                "{}: {}",
                case.id,
                case.source
            );
            assert_eq!(
                reference,
                reference_case_envelope(case, &result.identity),
                "{}: {}: {}",
                result.identity,
                case.id,
                case.source
            );
        }
        eprintln!(
            "Matched {} program outcomes, traces, and stdout against {}",
            cases.len(),
            result.identity
        );
    }
}

fn case_envelope(case: &Case, outcome: &str, trace: &str) -> String {
    format!(
        "{{\"id\":{},\"outcome\":{},\"trace\":{},\"stdout\":{}}}",
        json_string(&case.id),
        outcome,
        trace,
        case.stdout
    )
}

fn reference_case_envelope(case: &Case, identity: &str) -> String {
    let checkout = identity.contains("Checkout");
    let outcome = if !checkout && case.id == "set-literal-effects" {
        r#"{"status":"returned","value":{"$set":[1],"count":1}}"#
    } else if !checkout && case.id == "set-dynamic-collapse" {
        r#"{"status":"returned","value":{"$vector":[1,true]}}"#
    } else if case.id == "destructure-reify-pre-failure" {
        r#"{"status":"returned","value":9}"#
    } else {
        core_protocol_reference_outcome(case, identity)
    };
    let trace = if checkout && case.id == "destructure-defaults-order" {
        "[1,2,0]"
    } else if case.id == "destructure-reify-pre-failure" {
        "[9]"
    } else {
        &case.trace
    };
    case_envelope(case, outcome, trace)
}

#[test]
#[ignore = "requires Clojure CLI, Java, Node, and the pinned ClojureScript Maven dependencies"]
fn namespace_program_matches_standard_clojurescript_stdout() {
    let source_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/namespaces");
    let references = reference_results(
        "namespaces",
        &["program", source_root.to_str().unwrap(), "demo.main"],
        "",
    );
    let js = light_cljs::compiler::compile_namespace("demo.main", &[source_root]).unwrap();
    let actual = node(&js);
    for result in &references {
        let reference = &result.output;
        assert_eq!(reference, "4\n", "reference acceptance output");
        assert_eq!(
            &actual, reference,
            "{}: two-namespace acceptance output",
            result.identity
        );
        eprintln!(
            "light-cljs and {} produced exactly stdout 4\\n for demo.main",
            result.identity
        );
    }
}

#[test]
#[ignore = "requires Clojure CLI, Java, Node, and the pinned ClojureScript Maven dependencies"]
fn dependency_initialization_matches_standard_clojurescript() {
    let source_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/namespaces");
    let references = reference_results(
        "dependency-order",
        &["program", source_root.to_str().unwrap(), "order.main"],
        "",
    );
    let js = light_cljs::compiler::compile_namespace("order.main", &[source_root]).unwrap();
    let actual = node(&js);
    for result in &references {
        let reference = &result.output;
        assert_eq!(reference, "shared\nleft\nright\nmain\n10\n20\n7\n7\n40\n");
        assert_eq!(
            &actual, reference,
            "{}: diamond dependency order and global values",
            result.identity
        );
        eprintln!(
            "light-cljs and {} initialized the shared dependency once, in matching order",
            result.identity
        );
    }
}

#[test]
#[ignore = "requires Clojure CLI, Java, Node, and the pinned ClojureScript Maven dependencies"]
fn lexical_namespace_bindings_match_standard_clojurescript() {
    let source_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/namespaces");
    let references = reference_results(
        "lexical-namespaces",
        &["program", source_root.to_str().unwrap(), "lexical.main"],
        "",
    );
    let actual = node(&light_cljs::compile_namespace("lexical.main", &[source_root]).unwrap());
    for result in &references {
        assert_eq!(result.output, "40\n30\n12\n12\n11\n12\n30\n32\n33\n");
        assert_eq!(
            actual, result.output,
            "{}: lexical captures and qualified globals",
            result.identity
        );
        eprintln!(
            "light-cljs and {} matched lexical captures, aliases, namespace identity and redefinition",
            result.identity
        );
    }
}

#[test]
#[ignore = "requires Clojure CLI, Java, Node, and the pinned ClojureScript Maven dependencies"]
fn slash_bindings_match_standard_clojurescript_across_namespaces() {
    let source_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/namespaces");
    let references = reference_results(
        "slash-bindings",
        &["program", source_root.to_str().unwrap(), "boundary.main"],
        "",
    );
    let actual = node(&light_cljs::compile_namespace("boundary.main", &[source_root]).unwrap());
    for result in &references {
        assert_eq!(
            result.output, "7\n7\n8\n8\n9\n",
            "reference slash binding output"
        );
        assert_eq!(
            actual, result.output,
            "{}: slash binding identity",
            result.identity
        );
    }
}

#[test]
fn harness_preserves_symbol_list_seq_vector_and_nil_tags_independently() {
    let setup = r#"
        const branded = new WeakMap();
        const wrap = data => { const value = {}; branded.set(value, data); return value; };
        globalThis.lightInspectValue = value => branded.get(value) || null;
        const symbol = wrap({kind:'symbol', namespace:'a', name:'x', spelling:'a/x'});
        const empty = wrap({kind:'list', items:[]});
        const values = kind => wrap({kind, items:[symbol, empty]});
    "#;
    let script = String::from(setup)
        + &harness_script(&[(
            "seq-tags",
            "independent typed data",
            "observe => {observe(values('seq')); return [values('list'), values('vector'), empty, null];}",
        )]);
    assert_eq!(
        node(&script),
        concat!(
            r#"{"id":"seq-tags","outcome":{"status":"returned","value":[{"$list":[{"$symbol":{"namespace":"a","name":"x","spelling":"a/x"}},{"$list":[]}]},{"$vector":[{"$symbol":{"namespace":"a","name":"x","spelling":"a/x"}},{"$list":[]}]},{"$list":[]},null]},"trace":[{"$seq":[{"$symbol":{"namespace":"a","name":"x","spelling":"a/x"}},{"$list":[]}]}],"stdout":""}"#,
            "\n"
        )
    );
}

#[test]
fn map_encoder_checks_count_duplicates_types_and_unordered_entries_independently() {
    let setup = r#"
        const values = new WeakMap();
        const wrap = data => {const value = {}; values.set(value, data); return value;};
        globalThis.lightInspectValue = value => values.get(value) || null;
        const map = entries => wrap({kind:'map', entries, count:entries.length});
        const k = wrap({kind:'keyword', namespace:null, name:'a', spelling:'a'});
    "#;
    let script = String::from(setup)
        + &harness_script(&[(
            "map-tags",
            "independent map transport",
            "observe => { const a = map([[k, undefined], ['a', null], [1, false]]); const b = map([[1, false], ['a', null], [k, undefined]]); observe({$map: []}); return [a, b, map([])]; }",
        )]);
    let encoded = r#"{"$map":[["a",null],[1,false],[{"$keyword":{"namespace":null,"name":"a","spelling":"a"}},{"$value":"undefined"}]],"count":3}"#;
    assert_eq!(
        node(&script),
        format!(
            "{{\"id\":\"map-tags\",\"outcome\":{{\"status\":\"returned\",\"value\":[{encoded},{encoded},{{\"$map\":[],\"count\":0}}]}},\"trace\":[{{\"$object\":{{\"$map\":[]}}}}],\"stdout\":\"\"}}\n"
        )
    );
    for expression in [
        "map([[1, 2], [1, 3]])",
        "wrap({kind:'map', entries:[], count:1})",
        "wrap({kind:'map', entries:[[1]], count:1})",
        "wrap({kind:'map', entries:[[1,2,3]], count:1})",
        "wrap({kind:'map', entries:null, count:0})",
        "wrap({kind:'map', entries:Array(1), count:1})",
        "map([Array(2), [undefined, 1]])",
        "(() => {const a = {}; values.set(a,{kind:'map', entries:[[a,1]],count:1}); return a;})()",
        "map([[map([[1,2], [3,4]]),1], [map([[3,4], [1,2]]),2]])",
    ] {
        let script = String::from(setup)
            + &harness_script(&[(
                "invalid-map",
                "invalid adapter",
                &format!("() => {expression}"),
            )]);
        let error = try_run(node_command(), script.as_bytes(), Duration::from_secs(5))
            .expect_err("invalid maps fail outside compiled-case catches");
        assert!(
            fs::read(error._dir.0.join("stdout.txt"))
                .unwrap()
                .is_empty()
        );
    }
}

#[test]
fn map_source_order_and_numeric_literal_collisions_have_explicit_owned_semantics() {
    // Deliberate bounded-compiler semantics: upstream sometimes inlines hash-map
    // into separate key/value arrays, or trusts reader-distinct numeric constants
    // in unchecked literal arrays. Our constructor consistently uses JS equality
    // after evaluating all owned source children in their original order.
    let cases = [
        Case {
            id: "map-literal-number-collision".into(),
            source: "(let* [m {1 :int 1.0 :float}] [(count m) (= (get m 1) :float)])".into(),
            outcome: r#"{"status":"returned","value":{"$vector":[1,true]}}"#.into(),
            trace: "[]".into(),
            stdout: "\"\"".into(),
        },
        Case {
            id: "map-direct-constructor-order".into(),
            source: "(get (hash-map (observe 1) (observe 2) (observe 3) (observe 4)) 3)".into(),
            outcome: r#"{"status":"returned","value":4}"#.into(),
            trace: "[1,2,3,4]".into(),
            stdout: "\"\"".into(),
        },
        Case {
            id: "map-literal-children-order".into(),
            source: "(get {(observe 1) (observe 2) (observe 3) (observe 4)} 3)".into(),
            outcome: r#"{"status":"returned","value":4}"#.into(),
            trace: "[1,2,3,4]".into(),
            stdout: "\"\"".into(),
        },
    ];
    let actual = compiled_cases(&cases);
    let actual = outcomes(&actual, &cases).unwrap();
    for (case, actual) in cases.iter().zip(actual) {
        assert_eq!(
            actual,
            format!(
                "{{\"id\":{},\"outcome\":{},\"trace\":{},\"stdout\":{}}}",
                json_string(&case.id),
                case.outcome,
                case.trace,
                case.stdout
            )
        );
    }
}

#[test]
fn set_encoder_preserves_tags_checks_duplicates_and_sorts_only_members() {
    let setup = r#"
        const values = new WeakMap();
        const wrap = data => {const value = {}; values.set(value, data); return value;};
        globalThis.lightInspectValue = value => values.get(value) || null;
        const set = items => wrap({kind:'set', items, count:items.length});
        const seq = items => wrap({kind:'seq', items});
    "#;
    let script = String::from(setup)
        + &harness_script(&[(
            "set-tags",
            "independent unordered set observation",
            "observe => {observe({$set:[1]}); return [set([3,1,2]), set([2,3,1]), seq([3,1,2]), seq([2,3,1]), set([set([2,1]),null,false,undefined]), set([])];}",
        )]);
    assert_eq!(
        node(&script),
        concat!(
            "{\"id\":\"set-tags\",\"outcome\":{\"status\":\"returned\",\"value\":[",
            "{\"$set\":[1,2,3],\"count\":3},{\"$set\":[1,2,3],\"count\":3},",
            "{\"$seq\":[3,1,2]},{\"$seq\":[2,3,1]},",
            "{\"$set\":[false,null,{\"$set\":[1,2],\"count\":2},{\"$value\":\"undefined\"}],\"count\":4},",
            "{\"$set\":[],\"count\":0}]},\"trace\":[{\"$object\":{\"$set\":[1]}}],\"stdout\":\"\"}\n"
        )
    );
    for value in [
        "set([1,1])",
        "set([set([1,2]),set([2,1])])",
        "set(Array(2))",
        "wrap({kind:'set',items:[1],count:0})",
        "wrap({kind:'set',items:[],count:-1})",
        "wrap({kind:'set',items:[],count:0.5})",
        "wrap({kind:'set',items:null,count:0})",
        "(() => {const a = {}; values.set(a,{kind:'set',items:[a],count:1}); return a;})()",
    ] {
        let script = String::from(setup)
            + &harness_script(&[("invalid-set", "invalid adapter", &format!("() => {value}"))]);
        let error = try_run(node_command(), script.as_bytes(), Duration::from_secs(10))
            .expect_err("invalid sets fail outside compiled-case catches");
        assert!(
            fs::read(error._dir.0.join("stdout.txt"))
                .unwrap()
                .is_empty()
        );
    }
}

#[test]
fn set_reader_rejection_and_distinct_operand_effects_stay_separate() {
    for source in [
        "(count #{(observe 1) (observe 1)})",
        "(let* [x [1 2]] (count #{x x}))",
    ] {
        assert!(
            compile(&format!("(fn* [observe] {source})"))
                .unwrap_err()
                .to_string()
                .contains("duplicate set member")
        );
    }
    let cases = [
        Case {
            id: "set-literal-discard-order".into(),
            source: "(do #{(observe 1) (observe 2)} 3)".into(),
            outcome: r#"{"status":"returned","value":3}"#.into(),
            trace: "[1,2]".into(),
            stdout: "\"\"".into(),
        },
        Case {
            id: "set-literal-return-order".into(),
            source: "((fn* [] #{(observe 1) (observe 2)}))".into(),
            outcome: r#"{"status":"returned","value":{"$set":[1,2],"count":2}}"#.into(),
            trace: "[1,2]".into(),
            stdout: "\"\"".into(),
        },
        Case {
            id: "set-literal-failure-order".into(),
            source: "#{(observe 1) (js/lightHost.fail \"type\") (observe 3)}".into(),
            outcome: r#"{"status":"threw","error":{"category":"type-error"}}"#.into(),
            trace: r#"[1,["throw","type"]]"#.into(),
            stdout: "\"\"".into(),
        },
        Case {
            id: "set-direct-number-deduplication".into(),
            source: "[(count (hash-set 1 1.0)) (count (hash-set 0 -0.0))]".into(),
            outcome: r#"{"status":"returned","value":{"$vector":[1,1]}}"#.into(),
            trace: "[]".into(),
            stdout: "\"\"".into(),
        },
    ];
    let output = compiled_cases(&cases);
    let actual = outcomes(&output, &cases).unwrap();
    for (case, actual) in cases.iter().zip(actual) {
        assert_eq!(
            actual,
            format!(
                "{{\"id\":{},\"outcome\":{},\"trace\":{},\"stdout\":{}}}",
                json_string(&case.id),
                case.outcome,
                case.trace,
                case.stdout,
            )
        );
    }
}

#[test]
fn reduced_markers_are_opaque_harness_failures_even_when_nested_or_traced() {
    let runtime = include_str!("../src/js_runtime.js");
    for body in [
        "() => rt.core.reduced(1)",
        "() => rt.vector(rt.core.reduced(1))",
        "observe => { observe(rt.core.reduced(1)); return 0; }",
    ] {
        let script = format!(
            "const rt = {runtime}; globalThis.lightInspectValue = rt.inspect; {}",
            harness_script(&[("opaque-reduced", "Reduced cannot be a raw outcome", body)])
        );
        let error = try_run(node_command(), script.as_bytes(), Duration::from_secs(10))
            .expect_err("opaque markers must fail transport outside case catches");
        assert!(error.message.contains("unsupported opaque outcome value"));
        assert!(
            fs::read(error._dir.0.join("stdout.txt"))
                .unwrap()
                .is_empty()
        );
    }
}

#[test]
fn large_emitted_reduction_is_iterative_and_keeps_the_ordinary_call_abi() {
    let source = format!("(reduce + 0 [{}])", "1 ".repeat(150_000));
    let js = compile(&source).unwrap();
    assert_eq!(node(&format!("console.log({js});")), "150000\n");
}

fn collection_program_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/namespaces/collections")
}

const COLLECTION_PROGRAM_EXPECTED: &str = concat!(
    r#"{"id":"collection-program","outcome":{"status":"returned","value":{"$vector":[{"$vector":[1,2]},{"$vector":[1,2,3]},{"$keyword":{"namespace":null,"name":"pair","spelling":"pair"}},true]}},"trace":[{"$vector":[6,{"$symbol":{"namespace":"batch","name":"data","spelling":"batch/data"}},true,"batch.data"]},{"$vector":[{"$keyword":{"namespace":null,"name":"generated","spelling":"generated"}},{"$symbol":{"namespace":"batch","name":"data","spelling":"batch/data"}},{"$map":[[{"$keyword":{"namespace":null,"name":"nested","spelling":"nested"}},{"$list":[{"$symbol":{"namespace":null,"name":"x","spelling":"x"}},{"$vector":[1,2]}]}]],"count":1},{"$set":[{"$keyword":{"namespace":null,"name":"a","spelling":"a"}},{"$keyword":{"namespace":null,"name":"b","spelling":"b"}}],"count":2}]},true,"map","key","item",true],"stdout":"data\nops\nmain\n"}"#,
    "\n"
);

fn collection_program_driver(function: &str) -> String {
    harness_script(&[("collection-program", "batch.main/run", function)])
}

fn compiled_collection_program(js: &str) -> String {
    assert_eq!(js.matches("\n  const rt = ").count(), 1);
    let instrumented = js.replacen(
        " = // Original bounded runtime",
        " = globalThis.lightProgramRuntime = // Original bounded runtime",
        1,
    );
    node(&format!(
        "globalThis.lightInspectValue = value => lightProgramRuntime.inspect(value);\n{}",
        collection_program_driver(&format!(
            "observe => {{ const run = {instrumented}; return run(observe); }}"
        ))
    ))
}

#[test]
fn collection_namespace_macros_execute_typed_results_once_with_fresh_runtime_state() {
    let root = collection_program_root();
    let example = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/collections");
    for name in ["data.cljs", "ops.cljs", "main.cljs", "macros.clj"] {
        assert_eq!(
            fs::read(root.join("batch").join(name)).unwrap(),
            fs::read(example.join("batch").join(name)).unwrap(),
            "documented and differential programs must stay identical"
        );
    }
    let mut host = light_cljs::MacroHost::new();
    host.context_mut().set_macro_diagnostics(true);
    let mut outputs = Vec::new();
    for _ in 0..2 {
        let js = light_cljs::compile_namespace_with_macros(
            "batch.main",
            std::slice::from_ref(&root),
            &mut host,
        )
        .unwrap();
        assert_eq!(
            compiled_collection_program(&js),
            COLLECTION_PROGRAM_EXPECTED
        );
        outputs.push(js);
    }
    let stats = host.context_mut().macro_expansion_stats();
    for (name, count) in [("initial-data", 2), ("append-item", 4), ("literal-data", 4)] {
        let stat = stats
            .iter()
            .find(|stat| {
                stat.name == light_cljs::interpreter::Name::new(Some("batch.macros"), name)
            })
            .unwrap();
        assert_eq!(stat.expansions, count, "no duplicate expansions for {name}");
        assert!(stat.definition.is_some());
        assert!(stat.generated_forms > count);
    }
    // Two units in one host process retain separate globals, runtime brands and
    // immutable values. No observer/runtime must be installed for production JS.
    let isolation = format!(
        "const before = Reflect.ownKeys(globalThis); const a = {}; const b = {};\n\
         const x = a(x => x), y = a(x => x), z = b(x => x);\n\
         if (x === y || x === z || Object.isFrozen(x)) throw Error('shared result');\n\
         if (Reflect.ownKeys(globalThis).length !== before.length) throw Error('global install');\n\
         console.log('isolated');",
        outputs[0], outputs[1]
    );
    assert_eq!(
        node(&isolation),
        "data\nops\nmain\ndata\nops\nmain\nisolated\n"
    );
    let runtime_bytes = include_str!("../src/js_runtime.js").len();
    assert!(
        outputs
            .iter()
            .all(|js| js.len() > runtime_bytes && js.len() < runtime_bytes + 12_000)
    );
    let cli = run(
        {
            let mut command = Command::new(env!("CARGO_BIN_EXE_light-cljs"));
            command
                .args(["--compile", "--entry", "batch.main", "--source-root"])
                .arg(&example);
            command
        },
        Vec::new(),
        Duration::from_secs(30),
    );
    assert_eq!(
        compiled_collection_program(std::str::from_utf8(&cli.stdout).unwrap()),
        COLLECTION_PROGRAM_EXPECTED
    );
}

#[test]
#[ignore = "requires Clojure CLI, Java, Node, and the pinned ClojureScript Maven dependencies"]
fn collection_namespace_macros_match_standard_clojurescript_typed_outcomes() {
    let root = collection_program_root();
    let driver = format!(
        "globalThis.lightInspectValue = value => lightProgramInspectValue(value);\n{}",
        collection_program_driver("observe => { loadProgram(); return lightProgramRun(observe); }")
    );
    let references = reference_results_with_macro_roots(
        "collection-program",
        &["program", root.to_str().unwrap(), "batch.main", "run"],
        &driver,
        std::slice::from_ref(&root),
    );
    let js = light_cljs::compile_namespace("batch.main", &[root]).unwrap();
    let actual = compiled_collection_program(&js);
    for result in references {
        assert_eq!(
            result.output, COLLECTION_PROGRAM_EXPECTED,
            "{}: independent typed expectations",
            result.identity
        );
        assert_eq!(
            actual, result.output,
            "{}: complete macro/namespace collection program",
            result.identity
        );
        eprintln!(
            "Matched typed collection program result, report, nested generated data, ordered effects and fresh processes against {}",
            result.identity
        );
    }
}

#[test]
#[ignore = "requires both pinned ClojureScript references, Java and Node"]
fn reader_forms_match_pinned_values_effects_and_stdout() {
    let cases = cases()
        .into_iter()
        .filter(|case| {
            case.id.starts_with("reader-runtime-") || case.id.starts_with("reader-date-upstream-")
        })
        .collect::<Vec<_>>();
    let input = TempDir::new("reader-cases-input");
    let path = input.0.join("cases.tsv");
    fs::write(
        &path,
        cases
            .iter()
            .map(|case| format!("{}\t{}\n", case.id, case.source))
            .collect::<String>(),
    )
    .unwrap();
    let references = reference_results(
        "reader-cases",
        &["cases", path.to_str().unwrap()],
        include_str!("support/execution.js"),
    );
    let actual = compiled_cases(&cases);
    for reference in references {
        let observed = outcomes(&reference.output, &cases).unwrap();
        for ((case, local), oracle) in cases
            .iter()
            .zip(outcomes(&actual, &cases).unwrap())
            .zip(observed)
        {
            let expected = format!(
                "{{\"id\":{},\"outcome\":{},\"trace\":{},\"stdout\":{}}}",
                json_string(&case.id),
                case.outcome,
                case.trace,
                case.stdout
            );
            assert_eq!(local, expected, "{}: {}", case.id, case.source);
            assert_eq!(oracle, expected, "{}: {}", reference.identity, case.id);
        }
        eprintln!(
            "Matched {} reader outcomes, effects and stdout against {}",
            cases.len(),
            reference.identity
        );
    }
}

#[test]
#[ignore = "requires Clojure CLI, Java, Node, and both explicitly pinned references"]
fn shared_namespaces_and_macro_inference_match_standard_clojurescript() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/namespaces/shared");
    let actual =
        node(&light_cljs::compile_namespace("shared.main", std::slice::from_ref(&root)).unwrap());
    let expected = "leaf\nlogic\nmain\n7 7 9 13 13 6 8 9\n5 7 3 42 42 42 20 23 23 23 23\ntrue true true true new/ :shared.portable/key namespace doc 5\n";
    assert_eq!(actual, expected);
    for reference in reference_results_with_macro_roots(
        "shared-namespaces",
        &["program", root.to_str().unwrap(), "shared.main"],
        "",
        std::slice::from_ref(&root),
    ) {
        assert_eq!(reference.output, expected, "{}", reference.identity);
        assert_eq!(actual, reference.output, "{}", reference.identity);
        eprintln!(
            "{} matched CLJC imports, companion macros, source precedence, renamed identity and initialization order",
            reference.identity
        );
    }
}

#[test]
fn resolved_numeric_functions_evaluate_each_argument_once_in_order() {
    // This compiler's core is ordinary functions. Upstream's direct comparison
    // macros may elide/duplicate expressions; saved upstream functions are the
    // differential contract for evaluation order (see compiler-numeric.md).
    let js = compile(
        "(js/console.log (< (do (js/console.log 9) 9) (do (js/console.log 4) 4) (do (js/console.log 2) 2)))\n\
         (js/console.log (< (do (js/console.log 1) 1)))",
    )
    .unwrap();
    assert_eq!(node(&js), "9\n4\n2\nfalse\n1\ntrue\n");
}

#[test]
fn numeric_fixture_applications_match_expected_outcomes_in_quickjs_and_node() {
    use light_cljs::evaluator::{Evaluator, EvaluatorOptions};
    let numeric = cases()
        .into_iter()
        .filter(|case| case.id.starts_with("numeric-"))
        .collect::<Vec<_>>();
    let actual = compiled_cases(&numeric);
    for (index, (case, line)) in numeric
        .iter()
        .zip(outcomes(&actual, &numeric).unwrap())
        .enumerate()
    {
        assert_eq!(
            line,
            format!(
                "{{\"id\":{},\"outcome\":{},\"trace\":{},\"stdout\":{}}}",
                json_string(&case.id),
                case.outcome,
                case.trace,
                case.stdout
            )
        );
        let e = Evaluator::new(EvaluatorOptions::default()).unwrap();
        e.evaluate_js(&format!(
            "({})(rt)",
            include_str!("support/numeric_observation.js")
        ))
        .unwrap();
        let namespace = format!("semantic.case{index}");
        let source = format!(
            "(ns {namespace} (:require [cljs.core :as c]))\n(js/numericObservation (fn* [observe] {}))",
            case.source
        );
        let syntax = light_cljs::parse(&source).unwrap();
        let p = light_cljs::analyzer::analyze_namespaces(&[light_cljs::analyzer::NamespaceInput {
            name: &namespace,
            source: &source,
            syntax: &syntax,
            aliases: &[("c".into(), "cljs.core".into())],
            requires: &["cljs.core".into()],
        }])
        .unwrap();
        assert_eq!(
            e.string(e.evaluate(&p).unwrap()).unwrap(),
            format!(
                "{{\"outcome\":{},\"trace\":{},\"stdout\":{}}}",
                case.outcome, case.trace, case.stdout
            ),
            "{}",
            case.id
        );
    }
    assert_eq!(numeric.len(), 57);
}

#[test]
#[ignore = "requires Clojure CLI, Java, Node, and both explicitly pinned references"]
fn numeric_functions_match_pinned_values_effects_and_stdout() {
    let cases = cases()
        .into_iter()
        .filter(|case| case.id.starts_with("numeric-"))
        .collect::<Vec<_>>();
    let input = TempDir::new("numeric-cases-input");
    let path = input.0.join("cases.tsv");
    fs::write(
        &path,
        cases
            .iter()
            .map(|case| format!("{}\t{}\n", case.id, case.source))
            .collect::<String>(),
    )
    .unwrap();
    let references = reference_results(
        "numeric-cases",
        &["cases", path.to_str().unwrap()],
        include_str!("support/execution.js"),
    );
    let actual = compiled_cases(&cases);
    for reference in references {
        for ((case, local), oracle) in cases
            .iter()
            .zip(outcomes(&actual, &cases).unwrap())
            .zip(outcomes(&reference.output, &cases).unwrap())
        {
            let expected = format!(
                "{{\"id\":{},\"outcome\":{},\"trace\":{},\"stdout\":{}}}",
                json_string(&case.id),
                case.outcome,
                case.trace,
                case.stdout
            );
            assert_eq!(local, expected, "{}: {}", case.id, case.source);
            assert_eq!(oracle, expected, "{}: {}", reference.identity, case.id);
        }
        eprintln!(
            "Matched {} numeric/predicate outcomes, effects and stdout against {}",
            cases.len(),
            reference.identity
        );
    }
}

#[test]
fn compiled_overloads_expose_callable_entries_and_keep_large_rest_off_the_js_stack() {
    let js = light_cljs::compile("(fn ([x] x) ([x y] y))").unwrap();
    assert!(
        !js.contains("const rt ="),
        "fixed overloads need no runtime"
    );
    assert_eq!(
        node(&format!(
            r#"
      const assert = require('node:assert/strict');
      const f = {js};
      assert.equal(f.cljs$core$IFn$_invoke$arity$1(7), 7);
      assert.equal(f.cljs$core$IFn$_invoke$arity$2(2, 3), 3);
      assert.throws(() => f(), {{message: 'Invalid arity: 0'}});
      assert.throws(() => f(1, 2, 3), {{message: 'Invalid arity: 3'}});
      console.log('ok');
    "#
        )),
        "ok\n"
    );
    let js = light_cljs::compile("(fn [args] (apply (with-meta (fn [& xs] (count xs)) {}) args))")
        .unwrap();
    assert_eq!(
        node(&format!(
            "const f = {js}; console.log(f(Array(150000).fill(1)));"
        )),
        "150000\n"
    );
    let js = light_cljs::compile("(fn [& xs] xs)").unwrap();
    assert_eq!(
        node(&format!(
            r#"
      const assert = require('node:assert/strict');
      const f = {js}; const rest = f(1, 2, 3);
      assert.equal(f(), null);
      assert.equal(Array.isArray(rest), false);
      assert.equal(Object.isFrozen(rest), false);
      console.log('ok');
    "#
        )),
        "ok\n"
    );
}

#[test]
fn namespace_macro_generated_overloads_and_metadata_execute() {
    let root =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/namespaces/functions");
    let js = light_cljs::compile_namespace("functions.main", &[root]).unwrap();
    assert_eq!(node(&js), "10 13 10 13 true 100 11\n");
}

#[test]
#[ignore = "requires Clojure CLI, Java, Node, and both explicitly pinned references"]
fn compiled_overloads_match_pinned_values_effects_and_stdout() {
    let cases = cases()
        .into_iter()
        .filter(|case| case.id.starts_with("overload-") || case.id.starts_with("arity-"))
        .collect::<Vec<_>>();
    let input = TempDir::new("overload-cases-input");
    let path = input.0.join("cases.tsv");
    fs::write(
        &path,
        cases
            .iter()
            .map(|case| format!("{}\t{}\n", case.id, case.source))
            .collect::<String>(),
    )
    .unwrap();
    let actual = compiled_cases(&cases);
    for reference in reference_results(
        "overload-cases",
        &["cases", path.to_str().unwrap()],
        include_str!("support/execution.js"),
    ) {
        for ((case, local), oracle) in cases
            .iter()
            .zip(outcomes(&actual, &cases).unwrap())
            .zip(outcomes(&reference.output, &cases).unwrap())
        {
            let expected = format!(
                "{{\"id\":{},\"outcome\":{},\"trace\":{},\"stdout\":{}}}",
                json_string(&case.id),
                case.outcome,
                case.trace,
                case.stdout
            );
            assert_eq!(local, expected, "{}: {}", case.id, case.source);
            assert_eq!(oracle, expected, "{}: {}", reference.identity, case.id);
        }
        eprintln!(
            "Matched {} overload outcomes, effects and stdout against {}",
            cases.len(),
            reference.identity
        );
    }
    let root =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/namespaces/functions");
    let actual = node(
        &light_cljs::compile_namespace("functions.main", std::slice::from_ref(&root)).unwrap(),
    );
    for reference in reference_results_with_macro_roots(
        "overload-namespaces",
        &["program", root.to_str().unwrap(), "functions.main"],
        "",
        std::slice::from_ref(&root),
    ) {
        assert_eq!(actual, "10 13 10 13 true 100 11\n");
        assert_eq!(reference.output, actual, "{}", reference.identity);
    }
}

#[test]
#[ignore = "requires Clojure CLI, Java, Node, and both explicitly pinned references"]
fn compiled_invocation_matches_pinned_values_effects_and_stdout() {
    let cases = cases()
        .into_iter()
        .filter(|case| case.id.starts_with("invocation-"))
        .collect::<Vec<_>>();
    let input = TempDir::new("invocation-cases-input");
    let path = input.0.join("cases.tsv");
    fs::write(
        &path,
        cases
            .iter()
            .map(|case| format!("{}\t{}\n", case.id, case.source))
            .collect::<String>(),
    )
    .unwrap();
    let actual = compiled_cases(&cases);
    for reference in reference_results(
        "invocation-cases",
        &["cases", path.to_str().unwrap()],
        include_str!("support/execution.js"),
    ) {
        for ((case, local), oracle) in cases
            .iter()
            .zip(outcomes(&actual, &cases).unwrap())
            .zip(outcomes(&reference.output, &cases).unwrap())
        {
            let expected = format!(
                "{{\"id\":{},\"outcome\":{},\"trace\":{},\"stdout\":{}}}",
                json_string(&case.id),
                case.outcome,
                case.trace,
                case.stdout
            );
            assert_eq!(local, expected, "{}: {}", case.id, case.source);
            assert_eq!(oracle, expected, "{}: {}", reference.identity, case.id);
        }
        eprintln!(
            "Matched {} invocation outcomes, effects and stdout against {}",
            cases.len(),
            reference.identity
        );
    }
}

#[test]
fn invocation_runtime_obeys_receivers_and_large_lazy_arguments_without_the_encoder() {
    let numeric = compile("(fn [xs] (array (apply + xs) (count (apply vector xs)) (count (apply (with-meta (fn [& xs] xs) {}) xs))))").unwrap();
    let infinite = compile("(apply (fn [x & xs] (nth xs 2)) 9 (iterate inc 0))").unwrap();
    let short_circuit = compile("(apply < 10 (iterate inc 0))").unwrap();
    let callable = compile("(reify IFn (-invoke [self] self) (-invoke [self x] x))").unwrap();
    let metadata = compile("(with-meta (fn [& xs] (count xs)) {})").unwrap();
    let receiver = compile("(fn [] (array (let [] (this-as t t)) (do 1 (this-as t t))))").unwrap();
    assert_eq!(
        node(&format!(
            r#"
      const assert = require('node:assert/strict');
      const numeric = {numeric};
      assert.deepEqual(numeric(Array(150000).fill(1)), [150000, 150000, 150000]);
      assert.equal({infinite}, 2);
      assert.equal({short_circuit}, false);
      const callable = {callable};
      assert.equal(callable.call(null), callable);
      assert.equal(callable.apply(null, [9]), 9);
      assert.equal(callable.cljs$core$IFn$_invoke$arity$0(), callable);
      assert.throws(() => callable.call(null, 1, 2), {{message: 'Invalid arity: 2'}});
      const metadata = {metadata};
      assert.equal(typeof metadata, 'object');
      assert.equal(metadata.cljs$lang$applyTo, undefined);
      assert.equal(metadata.apply(null, Array(150000).fill(1)), 150000);
      const receiver = {receiver};
      assert.deepEqual(receiver.call({{}}), [globalThis, globalThis]);
      console.log('ok');
    "#
        )),
        "ok\n"
    );
}

#[test]
#[ignore = "requires Clojure CLI, Java, Node, and both explicitly pinned references"]
fn compiled_recur_matches_pinned_values_effects_and_stdout() {
    let cases = cases()
        .into_iter()
        .filter(|case| case.id.starts_with("recur-"))
        .collect::<Vec<_>>();
    let input = TempDir::new("recur-cases-input");
    let path = input.0.join("cases.tsv");
    fs::write(
        &path,
        cases
            .iter()
            .map(|case| format!("{}\t{}\n", case.id, case.source))
            .collect::<String>(),
    )
    .unwrap();
    let actual = compiled_cases(&cases);
    for reference in reference_results(
        "recur-cases",
        &["cases", path.to_str().unwrap()],
        include_str!("support/execution.js"),
    ) {
        for ((case, local), oracle) in cases
            .iter()
            .zip(outcomes(&actual, &cases).unwrap())
            .zip(outcomes(&reference.output, &cases).unwrap())
        {
            let expected = format!(
                "{{\"id\":{},\"outcome\":{},\"trace\":{},\"stdout\":{}}}",
                json_string(&case.id),
                case.outcome,
                case.trace,
                case.stdout
            );
            assert_eq!(local, expected, "{}: {}", case.id, case.source);
            assert_eq!(oracle, expected, "{}: {}", reference.identity, case.id);
        }
        eprintln!(
            "Matched {} recurrence outcomes, effects and stdout against {}",
            cases.len(),
            reference.identity
        );
    }
}

#[test]
fn recurrence_keeps_a_constant_stack_and_snapshot_closures_without_the_encoder() {
    let deep = compile("(fn [n] (if (pos? n) (recur (dec n)) n))").unwrap();
    let capture = compile(
        "(loop [i 0 fs (array)] (if (< i 4) (do (.push fs (fn [] i)) (recur (inc i) fs)) fs))",
    )
    .unwrap();
    let function_capture = compile("((fn [i fs] (let [x (* i 2)] (if (< i 4) (do (.push fs (fn [] (array i x))) (recur (inc i) fs)) fs))) 0 (array))").unwrap();
    let primitive = compile("(loop* [x 9] x)").unwrap();
    assert!(!primitive.contains("const rt ="));
    assert!(primitive.len() < 1000);
    assert_eq!(
        node(&format!(
            r#"
      const assert = require('node:assert/strict');
      assert.equal(({deep})(1000000), 0);
      assert.deepEqual(({capture}).map(f => f()), [0, 1, 2, 3]);
      assert.deepEqual(({function_capture}).map(f => f()), [[0, 0], [1, 2], [2, 4], [3, 6]]);
      assert.equal({primitive}, 9);
      console.log('ok');
    "#
        )),
        "ok\n"
    );
}

#[test]
fn try_analysis_follows_source_order_and_case_handles_vector_constants() {
    let code = compile("(try (def result 9) (throw :failure) (catch :default e result))").unwrap();
    assert_eq!(node(&format!("console.log({code});")), "9\n");
    let code = compile("(= (case [1] [1] [2] [3]) [2])").unwrap();
    assert_eq!(node(&format!("console.log({code});")), "true\n");
}

#[test]
#[ignore = "requires Clojure CLI, Java, Node, and both explicitly pinned references"]
fn compiled_letfn_matches_pinned_values_effects_and_stdout() {
    let cases = cases()
        .into_iter()
        .filter(|case| case.id.starts_with("letfn-"))
        .collect::<Vec<_>>();
    let input = TempDir::new("letfn-cases-input");
    let path = input.0.join("cases.tsv");
    fs::write(
        &path,
        cases
            .iter()
            .map(|case| format!("{}\t{}\n", case.id, case.source))
            .collect::<String>(),
    )
    .unwrap();
    let actual = compiled_cases(&cases);
    for reference in reference_results(
        "letfn-cases",
        &["cases", path.to_str().unwrap()],
        include_str!("support/execution.js"),
    ) {
        for ((case, local), oracle) in cases
            .iter()
            .zip(outcomes(&actual, &cases).unwrap())
            .zip(outcomes(&reference.output, &cases).unwrap())
        {
            let expected = format!(
                "{{\"id\":{},\"outcome\":{},\"trace\":{},\"stdout\":{}}}",
                json_string(&case.id),
                case.outcome,
                case.trace,
                case.stdout
            );
            assert_eq!(local, expected, "{}: {}", case.id, case.source);
            assert_eq!(oracle, expected, "{}: {}", reference.identity, case.id);
        }
        eprintln!(
            "Matched {} letfn outcomes, effects and stdout against {}",
            cases.len(),
            reference.identity
        );
    }
}

#[test]
fn letfn_retains_independent_groups_and_recur_iteration_captures_without_the_encoder() {
    let factory = compile(
        "(fn [x] (letfn [(f ([] (g)) ([n] (if (pos? n) (f (dec n)) (g)))) (g [] x)] (array f g)))",
    )
    .unwrap();
    let captures = compile("(loop [n 0 fs (array)] (if (< n 4) (letfn [(f [] (g)) (g [] n)] (.push fs f) (recur (inc n) fs)) fs))").unwrap();
    let primitive = compile("(letfn* [f (fn* [] (g)) g (fn* [] 9)] (f))").unwrap();
    assert!(!primitive.contains("const rt ="));
    assert!(primitive.len() < 1000);
    assert_eq!(
        node(&format!(
            r#"
      const assert = require('node:assert/strict');
      const make = {factory};
      const a = make(7), b = make(11);
      assert.equal(a[0](), 7);
      assert.equal(b[0](), 11);
      assert.equal(a[0](4), 7);
      assert.equal(b[1](), 11);
      assert.notEqual(a[0], b[0]);
      assert.deepEqual(({captures}).map(f => f()), [0, 1, 2, 3]);
      assert.equal({primitive}, 9);
      console.log('ok');
    "#
        )),
        "ok\n"
    );
}

// These are deliberately separate from the exact-conformance fixtures: both
// pinned CLJS emitters reuse function-scoped var slots for nested/iterated
// letfn groups. Our stable LocalId contract retains lexical group isolation.
fn letfn_identity_cases() -> Vec<Case> {
    [
        ("(letfn [(f [] (g)) (g [] 7)] (let [old f] (letfn [(f [] (g)) (g [] 9)] (array (old) (f)))))", "[7,9]"),
        ("(loop [n 0 fs []] (if (< n 4) (letfn [(f [] (g)) (g [] n)] (recur (inc n) (conj fs f))) (apply array (map (fn [f] (f)) fs))))", "[0,1,2,3]"),
    ].into_iter().enumerate().map(|(i, (source, value))| Case {
        id: format!("letfn-identity-{i}"), source: source.into(),
        outcome: format!("{{\"status\":\"returned\",\"value\":{value}}}"),
        trace: "[]".into(), stdout: "\"\"".into(),
    }).collect()
}

#[test]
fn letfn_identity_guarantees_do_not_reuse_nested_or_iterated_group_slots() {
    let cases = letfn_identity_cases();
    let output = compiled_cases(&cases);
    for (case, actual) in cases.iter().zip(outcomes(&output, &cases).unwrap()) {
        assert_eq!(
            actual,
            format!(
                "{{\"id\":{},\"outcome\":{},\"trace\":[],\"stdout\":\"\"}}",
                json_string(&case.id),
                case.outcome
            )
        );
    }
}

#[test]
#[ignore = "requires Clojure CLI, Java, Node, and both explicitly pinned references"]
fn pinned_letfn_var_reuse_differences_remain_explicit() {
    let cases = letfn_identity_cases();
    let input = TempDir::new("letfn-identities-input");
    let path = input.0.join("cases.tsv");
    fs::write(
        &path,
        cases
            .iter()
            .map(|case| format!("{}\t{}\n", case.id, case.source))
            .collect::<String>(),
    )
    .unwrap();
    let local = compiled_cases(&cases);
    for reference in reference_results(
        "letfn-identities",
        &["cases", path.to_str().unwrap()],
        include_str!("support/execution.js"),
    ) {
        for ((case, local), oracle) in cases
            .iter()
            .zip(outcomes(&local, &cases).unwrap())
            .zip(outcomes(&reference.output, &cases).unwrap())
        {
            assert_eq!(
                local,
                format!(
                    "{{\"id\":{},\"outcome\":{},\"trace\":[],\"stdout\":\"\"}}",
                    json_string(&case.id),
                    case.outcome
                )
            );
            assert_eq!(
                oracle,
                format!(
                    "{{\"id\":{},\"outcome\":{{\"status\":\"returned\",\"value\":{}}},\"trace\":[],\"stdout\":\"\"}}",
                    json_string(&case.id),
                    if case.id == "letfn-identity-0" {
                        "[9,9]"
                    } else {
                        "[3,3,3,3]"
                    }
                ),
                "{}: {}",
                reference.identity,
                case.id
            );
        }
        eprintln!(
            "Verified 2 explicit letfn var-reuse differences against {}; these are not conformance passes",
            reference.identity
        );
    }
}

#[test]
#[ignore = "requires Clojure CLI, Java, Node, and both explicitly pinned references"]
fn compiled_destructuring_matches_pinned_values_effects_and_stdout() {
    let cases = cases()
        .into_iter()
        .filter(|case| case.id.starts_with("destructure-") && !destructuring_difference(&case.id))
        .collect::<Vec<_>>();
    let input = TempDir::new("destructure-cases-input");
    let path = input.0.join("cases.tsv");
    fs::write(
        &path,
        cases
            .iter()
            .map(|case| format!("{}\t{}\n", case.id, case.source))
            .collect::<String>(),
    )
    .unwrap();
    let actual = compiled_cases(&cases);
    for reference in reference_results(
        "destructure-cases",
        &["cases", path.to_str().unwrap()],
        include_str!("support/execution.js"),
    ) {
        for ((case, local), oracle) in cases
            .iter()
            .zip(outcomes(&actual, &cases).unwrap())
            .zip(outcomes(&reference.output, &cases).unwrap())
        {
            let expected = format!(
                "{{\"id\":{},\"outcome\":{},\"trace\":{},\"stdout\":{}}}",
                json_string(&case.id),
                case.outcome,
                case.trace,
                case.stdout
            );
            assert_eq!(local, expected, "{}: {}", case.id, case.source);
            assert_eq!(oracle, expected, "{}: {}", reference.identity, case.id);
        }
        eprintln!(
            "Matched {} destructuring outcomes, effects and stdout against {}",
            cases.len(),
            reference.identity
        );
    }
}

fn destructuring_difference(id: &str) -> bool {
    matches!(
        id,
        "destructure-defaults-order" | "destructure-reify-pre-failure"
    )
}

#[test]
#[ignore = "requires Clojure CLI, Java, Node, and both explicitly pinned references"]
fn pinned_destructuring_differences_remain_explicit() {
    let cases: Vec<_> = cases()
        .into_iter()
        .filter(|c| destructuring_difference(&c.id))
        .collect();
    let input = TempDir::new("destructuring-differences");
    let path = input.0.join("cases.tsv");
    fs::write(
        &path,
        cases
            .iter()
            .map(|c| format!("{}\t{}\n", c.id, c.source))
            .collect::<String>(),
    )
    .unwrap();
    let local = compiled_cases(&cases);
    for reference in reference_results_policy(
        "destructuring-differences",
        &["cases", path.to_str().unwrap()],
        include_str!("support/execution.js"),
        &[],
        false,
    ) {
        for ((case, actual), oracle) in cases
            .iter()
            .zip(outcomes(&local, &cases).unwrap())
            .zip(outcomes(&reference.output, &cases).unwrap())
        {
            let envelope = |outcome: &str, trace: &str| {
                format!(
                    "{{\"id\":{},\"outcome\":{},\"trace\":{},\"stdout\":\"\"}}",
                    json_string(&case.id),
                    outcome,
                    trace
                )
            };
            assert_eq!(actual, envelope(&case.outcome, &case.trace));
            let (outcome, trace) = match case.id.as_str() {
                "destructure-defaults-order" => (
                    case.outcome.as_str(),
                    if reference.identity.contains("Checkout") {
                        "[1,2,0]"
                    } else {
                        "[0,1,2]"
                    },
                ),
                "destructure-reify-pre-failure" => (r#"{"status":"returned","value":9}"#, "[9]"),
                _ => unreachable!(),
            };
            assert_eq!(
                oracle,
                envelope(outcome, trace),
                "{}: {}",
                reference.identity,
                case.id
            );
        }
        eprintln!(
            "Verified four explicit destructuring differences against {}; these are not matching conformance cases",
            reference.identity
        );
    }
}

#[test]
fn destructuring_and_assertions_execute_without_the_shared_encoder() {
    let bindings =
        compile("(fn [input] (let [{:keys [a b c] :or {a 1 b 2 c 3}} input] (array a b c)))")
            .unwrap();
    let pre = compile("(fn [[x]] {:pre [(pos? x)] :post [(pos? %)]} (dec x))").unwrap();
    let methods = compile("(defprotocol P (run [this input])) (deftype T [a] P (run [_ [x]] (fn [] (+ a x)))) (let [r (run (T. 7) [5])] (r))").unwrap();
    let asserts = compile("(fn [test message] (assert (test) (message)))").unwrap();
    let body = compile("(fn [f] ((fn [x] {:post [(= % 4)]} (f) x) 4))").unwrap();
    assert_eq!(
        node(&format!(
            r#"
      const assert = require('node:assert/strict');
      const bindings = {bindings};
      assert.deepEqual(bindings(null), [1, 2, 3]);
      const pre = {pre};
      assert.throws(() => pre([0]), e => e instanceof Error && e.message === 'Assert failed: (pos? x)');
      assert.throws(() => pre([1]), e => e instanceof Error && e.message === 'Assert failed: (pos? %)');
      assert.equal(pre([3]), 2);
      assert.equal({methods}, 12);
      let effects = [];
      const check = {asserts};
      assert.equal(check(() => {{ effects.push(1); return 0; }}, () => effects.push(2)), null);
      assert.deepEqual(effects, [1]);
      assert.throws(() => check(() => {{ effects.push(3); return false; }}, () => {{ effects.push(4); return 'reason'; }}), /Assert failed: reason/);
      assert.deepEqual(effects, [1, 3, 4]);
      let count = 0;
      assert.equal(({body})(() => ++count), 4);
      assert.equal(count, 1);
      console.log('ok');
    "#
        )),
        "ok\n"
    );
}

#[test]
fn destructured_method_conditions_check_each_arity_and_receiver_context() {
    let mut sources = Vec::new();
    for kind in ["deftype", "defrecord"] {
        sources.push(format!("(defprotocol P (run [this input] [this input other])) ({kind} T [a] P (run [_ [x]] {{:pre [(pos? x)] :post [(pos? %)]}} (- x a)) (run [_ {{:keys [x]}} [y]] {{:pre [(pos? x) (pos? y)] :post [(pos? %)]}} (- (+ x y) a))) (let [t (->T 2)] (array (fn [x] (run t [x])) (fn [x y] (run t {{:x x}} [y]))))"));
    }
    sources.push("(defprotocol P (run [this input] [this input other])) (let [a 2 t (reify P (run [_ [x]] {:pre [(pos? x)] :post [(pos? %)]} (- x a)) (run [_ {:keys [x]} [y]] {:pre [(pos? x) (pos? y)] :post [(pos? %)]} (- (+ x y) a)))] (array (fn [x] (run t [x])) (fn [x y] (run t {:x x} [y]))))".into());
    sources.push("(let [a 2 t (reify IFn (-invoke [_ [x]] {:pre [(pos? x)] :post [(pos? %)]} (- x a)) (-invoke [_ {:keys [x]} [y]] {:pre [(pos? x) (pos? y)] :post [(pos? %)]} (- (+ x y) a)))] (array (fn [x] (t [x])) (fn [x y] (t {:x x} [y]))))".into());
    for (index, source) in sources.into_iter().enumerate() {
        let code = compile(&source).unwrap();
        if index < 2 {
            assert_eq!(
                node(&format!(
                    "const [one, two] = {code}; console.log(JSON.stringify([one(0), one(1), two(0, 4), two(3, 0), two(1, 1)]));"
                )),
                "[-2,-1,2,1,0]\n"
            );
            continue;
        }
        assert_eq!(
            node(&format!(
                r#"
          const assert = require('node:assert/strict');
          const [one, two] = {code};
          assert.equal(one(4), 2);
          assert.equal(two(3, 4), 5);
          for (const f of [() => one(0), () => one(1), () => two(0, 4), () => two(3, 0), () => two(1, 1)])
            assert.throws(f, e => e instanceof Error && e.message.startsWith('Assert failed: '));
          console.log('ok');
        "#
            )),
            "ok\n"
        );
    }
    let map = compile("(let [{:keys [a b c] :or {a 1 b 2 c 3}} {:a nil :b false}] (array a b c))")
        .unwrap();
    assert_eq!(
        node(&format!(
            "require('node:assert/strict').deepEqual({map}, [null, false, 3]); console.log('ok');"
        )),
        "ok\n"
    );
}

#[test]
#[ignore = "requires Clojure CLI, Java, Node, and both explicitly pinned references"]
fn compiled_exceptions_match_pinned_values_effects_and_stdout() {
    let cases = cases()
        .into_iter()
        .filter(|case| case.id.starts_with("exception-"))
        .collect::<Vec<_>>();
    let input = TempDir::new("exception-cases-input");
    let path = input.0.join("cases.tsv");
    fs::write(
        &path,
        cases
            .iter()
            .map(|case| format!("{}\t{}\n", case.id, case.source))
            .collect::<String>(),
    )
    .unwrap();
    let actual = compiled_cases(&cases);
    for reference in reference_results(
        "exception-cases",
        &["cases", path.to_str().unwrap()],
        include_str!("support/execution.js"),
    ) {
        for ((case, local), oracle) in cases
            .iter()
            .zip(outcomes(&actual, &cases).unwrap())
            .zip(outcomes(&reference.output, &cases).unwrap())
        {
            let expected = format!(
                "{{\"id\":{},\"outcome\":{},\"trace\":{},\"stdout\":{}}}",
                json_string(&case.id),
                case.outcome,
                case.trace,
                case.stdout
            );
            assert_eq!(local, expected, "{}: {}", case.id, case.source);
            assert_eq!(oracle, expected, "{}: {}", reference.identity, case.id);
        }
        eprintln!(
            "Matched {} exception outcomes, effects and stdout against {}",
            cases.len(),
            reference.identity
        );
    }
}

#[test]
fn exceptions_preserve_identity_and_abrupt_completion_without_the_encoder() {
    let code = compile("(fn [value effect] (try (throw value) (catch :default e (effect 1) (throw e)) (finally (effect 2))))").unwrap();
    let methods = compile("(defprotocol P (run [this])) (fn [cause] (let [data {:a 1} e (ex-info \"bad\" data cause) x (reify P (run [_] (throw e)))] (try (run x) (catch ExceptionInfo caught (array e caught (ex-data caught) data (ex-cause caught) (ex-message caught))))))").unwrap();
    let factory = compile("(fn [effect] (try (effect 1) (fn [] 7) (finally (effect 2))))").unwrap();
    let info = compile("(array ex-info ex-data ex-message ex-cause ExceptionInfo)").unwrap();
    assert_eq!(
        node(&format!(
            r#"
      const assert = require('node:assert/strict');
      const rethrow = {code};
      for (const value of [null, undefined, false, 0, '', 7, {{sentinel: true}}, new Error('original')]) {{
        const effects = []; let caught = false;
        try {{ rethrow(value, n => effects.push(n)); }} catch (error) {{ caught = true; assert.equal(error, value); }}
        assert.equal(caught, true); assert.deepEqual(effects, [1, 2]);
      }}
      const cause = new Error('root');
      const [original, caught, data, originalData, savedCause, message] = ({methods})(cause);
      assert.equal(caught, original); assert.equal(data, originalData); assert.equal(savedCause, cause);
      assert.equal(message, 'bad'); assert.ok(caught instanceof Error);
      const effects = []; const f = ({factory})(n => effects.push(n));
      assert.deepEqual(effects, [1, 2]); assert.equal(f(), 7);
      const [make, dataOf, messageOf, causeOf, Info] = {info};
      const e = make('message', null, undefined);
      assert.equal(e instanceof Info, true); assert.equal(e instanceof Error, true);
      assert.equal(e.name, 'Error'); assert.equal(messageOf(e), 'message');
      assert.equal(dataOf(e), null); assert.equal(causeOf(e), undefined);
      assert.throws(() => make(), /Invalid arity: 0/);
      console.log('ok');
    "#
        )),
        "ok\n"
    );
}

#[test]
#[ignore = "requires Clojure CLI, Java, Node, and both explicitly pinned references"]
fn compiled_control_matches_pinned_values_effects_and_stdout() {
    let cases = cases()
        .into_iter()
        .filter(|case| case.id.starts_with("control-"))
        .collect::<Vec<_>>();
    let input = TempDir::new("control-cases-input");
    let path = input.0.join("cases.tsv");
    fs::write(
        &path,
        cases
            .iter()
            .map(|case| format!("{}\t{}\n", case.id, case.source))
            .collect::<String>(),
    )
    .unwrap();
    let actual = compiled_cases(&cases);
    for reference in reference_results(
        "control-cases",
        &["cases", path.to_str().unwrap()],
        include_str!("support/execution.js"),
    ) {
        for ((case, local), oracle) in cases
            .iter()
            .zip(outcomes(&actual, &cases).unwrap())
            .zip(outcomes(&reference.output, &cases).unwrap())
        {
            let expected = format!(
                "{{\"id\":{},\"outcome\":{},\"trace\":{},\"stdout\":{}}}",
                json_string(&case.id),
                case.outcome,
                case.trace,
                case.stdout
            );
            assert_eq!(local, expected, "{}: {}", case.id, case.source);
            assert_eq!(oracle, expected, "{}: {}", reference.identity, case.id);
        }
        eprintln!(
            "Matched {} control outcomes, effects and stdout against {}",
            cases.len(),
            reference.identity
        );
    }
}

#[test]
#[ignore = "requires Clojure CLI, Java, Node, and both explicitly pinned references"]
fn compiled_metadata_match_pinned_values_effects_and_stdout() {
    let cases = cases()
        .into_iter()
        .filter(|case| case.id.starts_with("metadata-"))
        .collect::<Vec<_>>();
    let input = TempDir::new("metadata-cases-input");
    let path = input.0.join("cases.tsv");
    fs::write(
        &path,
        cases
            .iter()
            .map(|case| format!("{}\t{}\n", case.id, case.source))
            .collect::<String>(),
    )
    .unwrap();
    let actual = compiled_cases(&cases);
    for reference in reference_results(
        "metadata-cases",
        &["cases", path.to_str().unwrap()],
        include_str!("support/execution.js"),
    ) {
        for ((case, local), oracle) in cases
            .iter()
            .zip(outcomes(&actual, &cases).unwrap())
            .zip(outcomes(&reference.output, &cases).unwrap())
        {
            let expected = format!(
                "{{\"id\":{},\"outcome\":{},\"trace\":{},\"stdout\":{}}}",
                json_string(&case.id),
                case.outcome,
                case.trace,
                case.stdout
            );
            assert_eq!(local, expected, "{}: {}", case.id, case.source);
            assert_eq!(oracle, expected, "{}: {}", reference.identity, case.id);
        }
        eprintln!(
            "Matched {} metadata outcomes, effects and stdout against {}",
            cases.len(),
            reference.identity
        );
    }
}

#[test]
#[ignore = "requires Clojure CLI, Java, Node, and both explicitly pinned references"]
fn compiled_state_matches_pinned_values_effects_and_stdout() {
    let cases = cases()
        .into_iter()
        .filter(|case| case.id.starts_with("state-"))
        .collect::<Vec<_>>();
    let input = TempDir::new("state-cases-input");
    let path = input.0.join("cases.tsv");
    fs::write(
        &path,
        cases
            .iter()
            .map(|case| format!("{}\t{}\n", case.id, case.source))
            .collect::<String>(),
    )
    .unwrap();
    let actual = compiled_cases(&cases);
    for reference in reference_results(
        "state-cases",
        &["cases", path.to_str().unwrap()],
        include_str!("support/execution.js"),
    ) {
        for ((case, local), oracle) in cases
            .iter()
            .zip(outcomes(&actual, &cases).unwrap())
            .zip(outcomes(&reference.output, &cases).unwrap())
        {
            let expected = format!(
                "{{\"id\":{},\"outcome\":{},\"trace\":{},\"stdout\":{}}}",
                json_string(&case.id),
                case.outcome,
                case.trace,
                case.stdout
            );
            assert_eq!(local, expected, "{}: {}", case.id, case.source);
            assert_eq!(oracle, expected, "{}: {}", reference.identity, case.id);
        }
        eprintln!(
            "Matched {} state outcomes, effects and stdout against {}",
            cases.len(),
            reference.identity
        );
    }
}

#[test]
fn compiled_state_persists_across_namespaces() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/namespaces/state");
    let js = light_cljs::compile_namespace("state.main", &[root]).unwrap();
    assert_eq!(
        node(&js),
        "log 0 6\n6 6\n12 12 counter\ntrue 9\n9 [9 11] 11\n"
    );
}

#[test]
#[ignore = "requires Clojure CLI, Java, Node, and both explicitly pinned references"]
fn compiled_state_namespaces_match_pinned_clojurescript() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/namespaces/state");
    let actual =
        node(&light_cljs::compile_namespace("state.main", std::slice::from_ref(&root)).unwrap());
    let expected = "log 0 6\n6 6\n12 12 counter\ntrue 9\n9 [9 11] 11\n";
    assert_eq!(actual, expected);
    for reference in reference_results(
        "state-namespaces",
        &["program", root.to_str().unwrap(), "state.main"],
        "",
    ) {
        assert_eq!(reference.output, expected, "{}", reference.identity);
    }
}

#[test]
#[ignore = "requires Clojure CLI, Java, Node, and both explicitly pinned references"]
fn compiled_definitions_match_pinned_values_effects_and_stdout() {
    let cases = cases()
        .into_iter()
        .filter(|case| case.id.starts_with("definitions-"))
        .collect::<Vec<_>>();
    let input = TempDir::new("definitions-cases-input");
    let path = input.0.join("cases.tsv");
    fs::write(
        &path,
        cases
            .iter()
            .map(|case| format!("{}\t{}\n", case.id, case.source))
            .collect::<String>(),
    )
    .unwrap();
    let actual = compiled_cases(&cases);
    for reference in reference_results(
        "definitions-cases",
        &["cases", path.to_str().unwrap()],
        include_str!("support/execution.js"),
    ) {
        for ((case, local), oracle) in cases
            .iter()
            .zip(outcomes(&actual, &cases).unwrap())
            .zip(outcomes(&reference.output, &cases).unwrap())
        {
            let expected = format!(
                "{{\"id\":{},\"outcome\":{},\"trace\":{},\"stdout\":{}}}",
                json_string(&case.id),
                case.outcome,
                case.trace,
                case.stdout
            );
            assert_eq!(local, expected, "{}: {}", case.id, case.source);
            assert_eq!(oracle, expected, "{}: {}", reference.identity, case.id);
        }
        eprintln!(
            "Matched {} definition outcomes, effects and stdout against {}",
            cases.len(),
            reference.identity
        );
    }
}

#[test]
fn compiled_definitions_preserve_namespace_macro_and_callback_scopes() {
    let root =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/namespaces/definitions");
    let actual = node(&light_cljs::compile_namespace("definitions.main", &[root]).unwrap());
    assert_eq!(
        actual,
        "7 nil false\n[2 2] 1\n[true true] false\n3 4\n[4 9 9] [4 4 4]\ntrue true :value\n8 1\n11 11\n2 1\n"
    );
    // Synchronous binding restores before Promise callbacks run. Captured locals
    // remain snapshots; global reads see the restored value.
    let js = compile("(def ^:dynamic x 1) (binding [x 2] (let [snapshot x] (.then (.resolve js/Promise) (fn [] (js/console.log snapshot x)))))").unwrap();
    assert_eq!(node(&js), "2 1\n");
}

#[test]
#[ignore = "requires Clojure CLI, Java, Node, and both explicitly pinned references"]
fn compiled_definitions_namespaces_match_pinned_clojurescript() {
    let root =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/namespaces/definitions");
    let actual = node(
        &light_cljs::compile_namespace("definitions.main", std::slice::from_ref(&root)).unwrap(),
    );
    let expected = "7 nil false\n[2 2] 1\n[true true] false\n3 4\n[4 9 9] [4 4 4]\ntrue true :value\n8 1\n11 11\n2 1\n";
    assert_eq!(actual, expected);
    for reference in reference_results_with_macro_roots(
        "definitions-namespaces",
        &["program", root.to_str().unwrap(), "definitions.main"],
        "",
        std::slice::from_ref(&root),
    ) {
        assert_eq!(reference.output, expected, "{}", reference.identity);
    }
}

#[test]
#[ignore = "requires Clojure CLI, Java, Node, and both explicitly pinned references"]
fn compiled_interop_match_pinned_values_effects_and_stdout() {
    let cases = cases()
        .into_iter()
        .filter(|case| case.id.starts_with("interop-"))
        .collect::<Vec<_>>();
    let input = TempDir::new("interop-cases-input");
    let path = input.0.join("cases.tsv");
    fs::write(
        &path,
        cases
            .iter()
            .map(|case| format!("{}\t{}\n", case.id, case.source))
            .collect::<String>(),
    )
    .unwrap();
    let actual = compiled_cases(&cases);
    for reference in reference_results(
        "interop-cases",
        &["cases", path.to_str().unwrap()],
        include_str!("support/execution.js"),
    ) {
        for ((case, local), oracle) in cases
            .iter()
            .zip(outcomes(&actual, &cases).unwrap())
            .zip(outcomes(&reference.output, &cases).unwrap())
        {
            let expected = format!(
                "{{\"id\":{},\"outcome\":{},\"trace\":{},\"stdout\":{}}}",
                json_string(&case.id),
                case.outcome,
                case.trace,
                case.stdout
            );
            assert_eq!(local, expected, "{}: {}", case.id, case.source);
            assert_eq!(oracle, expected, "{}: {}", reference.identity, case.id);
        }
        eprintln!(
            "Matched {} interop outcomes, effects and stdout against {}",
            cases.len(),
            reference.identity
        );
    }
}

#[test]
fn compiled_interop_preserves_host_results_effects_and_failures() {
    let cases = cases()
        .into_iter()
        .filter(|case| case.id.starts_with("interop-"))
        .collect::<Vec<_>>();
    let actual = compiled_cases(&cases);
    for (case, observed) in cases.iter().zip(outcomes(&actual, &cases).unwrap()) {
        let expected = format!(
            "{{\"id\":{},\"outcome\":{},\"trace\":{},\"stdout\":{}}}",
            json_string(&case.id),
            case.outcome,
            case.trace,
            case.stdout
        );
        assert_eq!(observed, expected, "{}: {}", case.id, case.source);
    }
}

#[test]
#[ignore = "requires Clojure CLI, Java, Node, and both explicitly pinned references"]
fn compiled_protocols_match_pinned_values_effects_and_stdout() {
    let cases = cases()
        .into_iter()
        .filter(|case| case.id.starts_with("protocol-"))
        .collect::<Vec<_>>();
    let input = TempDir::new("protocol-cases-input");
    let path = input.0.join("cases.tsv");
    fs::write(
        &path,
        cases
            .iter()
            .map(|case| format!("{}\t{}\n", case.id, case.source))
            .collect::<String>(),
    )
    .unwrap();
    let actual = compiled_cases(&cases);
    for reference in reference_results(
        "protocol-cases",
        &["cases", path.to_str().unwrap()],
        include_str!("support/execution.js"),
    ) {
        for ((case, local), oracle) in cases
            .iter()
            .zip(outcomes(&actual, &cases).unwrap())
            .zip(outcomes(&reference.output, &cases).unwrap())
        {
            let expected = format!(
                "{{\"id\":{},\"outcome\":{},\"trace\":{},\"stdout\":{}}}",
                json_string(&case.id),
                case.outcome,
                case.trace,
                case.stdout
            );
            assert_eq!(local, expected, "{}: {}", case.id, case.source);
            assert_eq!(oracle, expected, "{}: {}", reference.identity, case.id);
        }
        eprintln!(
            "Matched {} protocol outcomes, effects and stdout against {}",
            cases.len(),
            reference.identity
        );
    }
}

#[test]
fn compiled_protocols_persist_across_namespaces() {
    let root =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/namespaces/protocols");
    let js = light_cljs::compile_namespace("protocols.main", &[root]).unwrap();
    assert_eq!(
        node(&js),
        "[7 10 :alpha :beta]\n[true true true false]\n:meta\nRun alpha P\n12 false true\n"
    );
}

#[test]
#[ignore = "requires Clojure CLI, Java, Node, and both explicitly pinned references"]
fn compiled_protocols_namespaces_match_pinned_clojurescript() {
    let root =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/namespaces/protocols");
    let actual = node(
        &light_cljs::compile_namespace("protocols.main", std::slice::from_ref(&root)).unwrap(),
    );
    let expected =
        "[7 10 :alpha :beta]\n[true true true false]\n:meta\nRun alpha P\n12 false true\n";
    assert_eq!(actual, expected);
    for reference in reference_results(
        "protocol-namespaces",
        &["program", root.to_str().unwrap(), "protocols.main"],
        "",
    ) {
        assert_eq!(reference.output, expected, "{}", reference.identity);
    }
}

#[test]
#[ignore = "requires Clojure CLI, Java, Node, and both explicitly pinned references"]
fn compiled_reify_match_pinned_values_effects_and_stdout() {
    let cases = cases()
        .into_iter()
        .filter(|case| case.id.starts_with("reify-"))
        .collect::<Vec<_>>();
    let input = TempDir::new("reify-cases-input");
    let path = input.0.join("cases.tsv");
    fs::write(
        &path,
        cases
            .iter()
            .map(|case| format!("{}\t{}\n", case.id, case.source))
            .collect::<String>(),
    )
    .unwrap();
    let actual = compiled_cases(&cases);
    for reference in reference_results(
        "reify-cases",
        &["cases", path.to_str().unwrap()],
        include_str!("support/execution.js"),
    ) {
        for ((case, local), oracle) in cases
            .iter()
            .zip(outcomes(&actual, &cases).unwrap())
            .zip(outcomes(&reference.output, &cases).unwrap())
        {
            let expected = format!(
                "{{\"id\":{},\"outcome\":{},\"trace\":{},\"stdout\":{}}}",
                json_string(&case.id),
                case.outcome,
                case.trace,
                case.stdout
            );
            assert_eq!(local, expected, "{}: {}", case.id, case.source);
            assert_eq!(oracle, expected, "{}: {}", reference.identity, case.id);
        }
        eprintln!(
            "Matched {} reify outcomes, effects and stdout against {}",
            cases.len(),
            reference.identity
        );
    }
}

#[test]
fn compiled_reify_event_objects_cross_namespace_aliases() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/namespaces/reify");
    let js = light_cljs::compile_namespace("reify-events.main", &[root]).unwrap();
    assert_eq!(node(&js), "events-ok {:total 13} [[3 13 1 true]]\n");
}

#[test]
#[ignore = "requires Clojure CLI, Java, Node, and both explicitly pinned references"]
fn compiled_reify_events_match_pinned_clojurescript() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/namespaces/reify");
    let expected = "events-ok {:total 13} [[3 13 1 true]]\n";
    assert_eq!(
        node(
            &light_cljs::compile_namespace("reify-events.main", std::slice::from_ref(&root))
                .unwrap()
        ),
        expected
    );
    for reference in reference_results(
        "reify-events",
        &["program", root.to_str().unwrap(), "reify-events.main"],
        "",
    ) {
        assert_eq!(reference.output, expected, "{}", reference.identity);
    }
}

#[test]
#[ignore = "requires Clojure CLI, Java, Node, and both explicitly pinned references"]
fn compiled_deftype_match_pinned_values_effects_and_stdout() {
    let cases = cases()
        .into_iter()
        .filter(|case| case.id.starts_with("deftype-"))
        .collect::<Vec<_>>();
    let input = TempDir::new("deftype-cases-input");
    let path = input.0.join("cases.tsv");
    fs::write(
        &path,
        cases
            .iter()
            .map(|case| format!("{}\t{}\n", case.id, case.source))
            .collect::<String>(),
    )
    .unwrap();
    let actual = compiled_cases(&cases);
    for reference in reference_results(
        "deftype-cases",
        &["cases", path.to_str().unwrap()],
        include_str!("support/execution.js"),
    ) {
        for ((case, local), oracle) in cases
            .iter()
            .zip(outcomes(&actual, &cases).unwrap())
            .zip(outcomes(&reference.output, &cases).unwrap())
        {
            let expected = format!(
                "{{\"id\":{},\"outcome\":{},\"trace\":{},\"stdout\":{}}}",
                json_string(&case.id),
                case.outcome,
                case.trace,
                case.stdout
            );
            assert_eq!(local, expected, "{}: {}", case.id, case.source);
            assert_eq!(oracle, expected, "{}: {}", reference.identity, case.id);
        }
        eprintln!(
            "Matched {} deftype outcomes, effects and stdout against {}",
            cases.len(),
            reference.identity
        );
    }
}

#[test]
fn compiled_deftype_namespace_identity_and_dispatch() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/namespaces/types");
    assert_eq!(
        node(&light_cljs::compile_namespace("types.main", &[root]).unwrap()),
        "types-ok\n"
    );
}

#[test]
#[ignore = "requires Clojure CLI, Java, Node, and both explicitly pinned references"]
fn compiled_deftype_namespaces_match_pinned_clojurescript() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/namespaces/types");
    assert_eq!(
        node(&light_cljs::compile_namespace("types.main", std::slice::from_ref(&root)).unwrap()),
        "types-ok\n"
    );
    for reference in reference_results(
        "deftype-namespaces",
        &["program", root.to_str().unwrap(), "types.main"],
        "",
    ) {
        assert_eq!(reference.output, "types-ok\n", "{}", reference.identity);
    }
}

#[test]
#[ignore = "requires Clojure CLI, Java, Node, and both explicitly pinned references"]
fn compiled_record_match_pinned_values_effects_and_stdout() {
    let cases = cases()
        .into_iter()
        .filter(|case| case.id.starts_with("record-"))
        .collect::<Vec<_>>();
    let input = TempDir::new("record-cases-input");
    let path = input.0.join("cases.tsv");
    fs::write(
        &path,
        cases
            .iter()
            .map(|case| format!("{}\t{}\n", case.id, case.source))
            .collect::<String>(),
    )
    .unwrap();
    let actual = compiled_cases(&cases);
    for reference in reference_results(
        "record-cases",
        &["cases", path.to_str().unwrap()],
        include_str!("support/execution.js"),
    ) {
        for ((case, local), oracle) in cases
            .iter()
            .zip(outcomes(&actual, &cases).unwrap())
            .zip(outcomes(&reference.output, &cases).unwrap())
        {
            let expected = format!(
                "{{\"id\":{},\"outcome\":{},\"trace\":{},\"stdout\":{}}}",
                json_string(&case.id),
                case.outcome,
                case.trace,
                case.stdout
            );
            assert_eq!(local, expected, "{}: {}", case.id, case.source);
            assert_eq!(oracle, expected, "{}: {}", reference.identity, case.id);
        }
        eprintln!(
            "Matched {} record outcomes, effects and stdout against {}",
            cases.len(),
            reference.identity
        );
    }
}

#[test]
fn compiled_record_namespace_identity_and_dispatch() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/namespaces/records");
    assert_eq!(
        node(&light_cljs::compile_namespace("records.main", &[root]).unwrap()),
        "records-ok\n"
    );
}

#[test]
#[ignore = "requires Clojure CLI, Java, Node, and both explicitly pinned references"]
fn compiled_record_namespaces_match_pinned_clojurescript() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/namespaces/records");
    assert_eq!(
        node(&light_cljs::compile_namespace("records.main", std::slice::from_ref(&root)).unwrap()),
        "records-ok\n"
    );
    for reference in reference_results(
        "record-namespaces",
        &["program", root.to_str().unwrap(), "records.main"],
        "",
    ) {
        assert_eq!(reference.output, "records-ok\n", "{}", reference.identity);
    }
}

#[test]
fn compiled_extension_outcomes_in_fresh_processes() {
    for case in cases()
        .into_iter()
        .filter(|case| case.id.starts_with("extension-"))
    {
        let actual = compiled_cases(std::slice::from_ref(&case));
        let expected = format!(
            "{{\"id\":{},\"outcome\":{},\"trace\":{},\"stdout\":{}}}",
            json_string(&case.id),
            case.outcome,
            case.trace,
            case.stdout
        );
        assert_eq!(
            outcomes(&actual, std::slice::from_ref(&case)).unwrap()[0],
            expected,
            "{}",
            case.id
        );
    }
}

#[test]
#[ignore = "requires Clojure CLI, Java, Node, and both explicitly pinned references"]
fn compiled_extension_matches_pinned_isolated_references() {
    let cases = cases()
        .into_iter()
        .filter(|case| case.id.starts_with("extension-"))
        .collect::<Vec<_>>();
    let input = TempDir::new("extension-cases-input");
    let path = input.0.join("cases.tsv");
    fs::write(
        &path,
        cases
            .iter()
            .map(|case| format!("{}\t{}\n", case.id, case.source))
            .collect::<String>(),
    )
    .unwrap();
    let driver = format!(
        "globalThis.lightExecutionDriver = {};\n{}",
        json_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/support/execution.js"
        )),
        include_str!("support/isolated_execution.js")
    );
    for reference in reference_results(
        "extension-cases",
        &["cases", path.to_str().unwrap()],
        &driver,
    ) {
        for (case, oracle) in cases
            .iter()
            .zip(outcomes(&reference.output, &cases).unwrap())
        {
            let expected = format!(
                "{{\"id\":{},\"outcome\":{},\"trace\":{},\"stdout\":{}}}",
                json_string(&case.id),
                case.outcome,
                case.trace,
                case.stdout
            );
            assert_eq!(oracle, expected, "{}: {}", reference.identity, case.id);
        }
        eprintln!(
            "Matched {} isolated extension outcomes, effects and stdout against {}",
            cases.len(),
            reference.identity
        );
    }
}

#[test]
fn compiled_extension_namespace_aliases() {
    let root =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/namespaces/extensions");
    assert_eq!(
        node(&light_cljs::compile_namespace("extensions.main", &[root]).unwrap()),
        "extensions-ok\n"
    );
}

#[test]
#[ignore = "requires Clojure CLI, Java, Node, and both explicitly pinned references"]
fn compiled_extension_namespaces_match_pinned_references() {
    let root =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/namespaces/extensions");
    for reference in reference_results(
        "extension-namespaces",
        &["program", root.to_str().unwrap(), "extensions.main"],
        "",
    ) {
        assert_eq!(
            reference.output, "extensions-ok\n",
            "{}",
            reference.identity
        );
    }
}

#[test]
fn compiled_core_protocol_outcomes_in_fresh_processes() {
    for case in cases()
        .into_iter()
        .filter(|case| case.id.starts_with("core-protocol-"))
    {
        let actual = compiled_cases(std::slice::from_ref(&case));
        let expected = format!(
            "{{\"id\":{},\"outcome\":{},\"trace\":{},\"stdout\":{}}}",
            json_string(&case.id),
            case.outcome,
            case.trace,
            case.stdout
        );
        assert_eq!(
            outcomes(&actual, std::slice::from_ref(&case)).unwrap()[0],
            expected,
            "{}",
            case.id
        );
    }
}

#[test]
#[ignore = "requires Clojure CLI, Java, Node, and both explicitly pinned references"]
fn compiled_core_protocol_matches_pinned_isolated_references() {
    let cases = cases()
        .into_iter()
        .filter(|case| case.id.starts_with("core-protocol-"))
        .collect::<Vec<_>>();
    let input = TempDir::new("core-protocol-cases-input");
    let path = input.0.join("cases.tsv");
    fs::write(
        &path,
        cases
            .iter()
            .map(|case| format!("{}\t{}\n", case.id, case.source))
            .collect::<String>(),
    )
    .unwrap();
    let driver = format!(
        "globalThis.lightExecutionDriver = {};\n{}",
        json_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/support/execution.js"
        )),
        include_str!("support/isolated_execution.js")
    );
    for reference in reference_results_policy(
        "core-protocol-cases",
        &["cases", path.to_str().unwrap()],
        &driver,
        &[],
        false,
    ) {
        for (case, oracle) in cases
            .iter()
            .zip(outcomes(&reference.output, &cases).unwrap())
        {
            let expected = format!(
                "{{\"id\":{},\"outcome\":{},\"trace\":{},\"stdout\":{}}}",
                json_string(&case.id),
                core_protocol_reference_outcome(case, &reference.identity),
                case.trace,
                case.stdout
            );
            assert_eq!(oracle, expected, "{}: {}", reference.identity, case.id);
        }
        eprintln!(
            "Matched {} isolated core protocol outcomes, effects and stdout against {}",
            cases.len(),
            reference.identity
        );
    }
}

// The pinned checkout changed fast protocol dispatch: missing declared methods
// throw Error there, but release 1.12.134 emits an unchecked slot call (TypeError).
fn core_protocol_reference_outcome<'a>(case: &'a Case, identity: &str) -> &'a str {
    if !identity.contains("Checkout")
        && matches!(
            case.id.as_str(),
            "core-protocol-marker-lookup-missing"
                | "core-protocol-marker-seq-missing"
                | "core-protocol-marker-count-missing"
                | "core-protocol-marker-reduce-missing"
                | "core-protocol-partial-reduce-missing"
                | "core-protocol-partial-lookup-missing"
                | "core-protocol-partial-indexed-missing"
                | "core-protocol-marker-empty-missing"
                | "core-protocol-marker-next-missing"
                | "core-protocol-marker-default-hash-equivalence"
        )
    {
        r#"{"status":"threw","error":{"category":"type-error"}}"#
    } else {
        &case.outcome
    }
}

#[test]
#[ignore = "requires Clojure CLI, Java, Node, and both explicitly pinned references"]
fn compiled_helpers_match_pinned_values_effects_and_stdout() {
    let cases = cases()
        .into_iter()
        .filter(|case| case.id.starts_with("helper-"))
        .collect::<Vec<_>>();
    let input = TempDir::new("helper-cases-input");
    let path = input.0.join("cases.tsv");
    fs::write(
        &path,
        cases
            .iter()
            .map(|case| format!("{}\t{}\n", case.id, case.source))
            .collect::<String>(),
    )
    .unwrap();
    let actual = compiled_cases(&cases);
    for reference in reference_results(
        "helper-cases",
        &["cases", path.to_str().unwrap()],
        include_str!("support/execution.js"),
    ) {
        for ((case, local), oracle) in cases
            .iter()
            .zip(outcomes(&actual, &cases).unwrap())
            .zip(outcomes(&reference.output, &cases).unwrap())
        {
            let expected = format!(
                "{{\"id\":{},\"outcome\":{},\"trace\":{},\"stdout\":{}}}",
                json_string(&case.id),
                case.outcome,
                case.trace,
                case.stdout
            );
            assert_eq!(local, expected, "{}: {}", case.id, case.source);
            assert_eq!(oracle, expected, "{}: {}", reference.identity, case.id);
        }
        eprintln!(
            "Matched {} helper outcomes, effects and stdout against {}",
            cases.len(),
            reference.identity
        );
    }
}

#[test]
fn compiled_helpers_preserve_values_effects_and_failures() {
    let cases = cases()
        .into_iter()
        .filter(|case| case.id.starts_with("helper-"))
        .collect::<Vec<_>>();
    let actual = compiled_cases(&cases);
    let mut failures = Vec::new();
    for (case, actual) in cases.iter().zip(outcomes(&actual, &cases).unwrap()) {
        let expected = format!(
            "{{\"id\":{},\"outcome\":{},\"trace\":{},\"stdout\":{}}}",
            json_string(&case.id),
            case.outcome,
            case.trace,
            case.stdout
        );
        if actual != expected {
            failures.push(format!(
                "{}: {}\nexpected {}\nactual {}",
                case.id, case.source, expected, actual
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
#[ignore = "requires Clojure CLI, Java, Node, and both explicitly pinned references"]
fn compiled_deferred_match_pinned_values_effects_and_stdout() {
    let cases = cases()
        .into_iter()
        .filter(|case| case.id.starts_with("deferred-"))
        .collect::<Vec<_>>();
    let input = TempDir::new("deferred-cases-input");
    let path = input.0.join("cases.tsv");
    fs::write(
        &path,
        cases
            .iter()
            .map(|case| format!("{}\t{}\n", case.id, case.source))
            .collect::<String>(),
    )
    .unwrap();
    let actual = compiled_cases(&cases);
    for reference in reference_results(
        "deferred-cases",
        &["cases", path.to_str().unwrap()],
        include_str!("support/execution.js"),
    ) {
        for ((case, local), oracle) in cases
            .iter()
            .zip(outcomes(&actual, &cases).unwrap())
            .zip(outcomes(&reference.output, &cases).unwrap())
        {
            let expected = format!(
                "{{\"id\":{},\"outcome\":{},\"trace\":{},\"stdout\":{}}}",
                json_string(&case.id),
                case.outcome,
                case.trace,
                case.stdout
            );
            assert_eq!(local, expected, "{}: {}", case.id, case.source);
            assert_eq!(oracle, expected, "{}: {}", reference.identity, case.id);
        }
        eprintln!(
            "Matched {} deferred outcomes, effects and stdout against {}",
            cases.len(),
            reference.identity
        );
    }
}

#[test]
#[ignore = "requires Clojure CLI, Java, Node, and both explicitly pinned references"]
fn compiled_sequences_match_pinned_values_effects_and_stdout() {
    let cases = cases()
        .into_iter()
        .filter(|case| case.id.starts_with("sequences-"))
        .collect::<Vec<_>>();
    let input = TempDir::new("sequences-cases-input");
    let path = input.0.join("cases.tsv");
    fs::write(
        &path,
        cases
            .iter()
            .map(|case| format!("{}\t{}\n", case.id, case.source))
            .collect::<String>(),
    )
    .unwrap();
    let actual = compiled_cases(&cases);
    for reference in reference_results(
        "sequences-cases",
        &["cases", path.to_str().unwrap()],
        include_str!("support/execution.js"),
    ) {
        for ((case, local), oracle) in cases
            .iter()
            .zip(outcomes(&actual, &cases).unwrap())
            .zip(outcomes(&reference.output, &cases).unwrap())
        {
            let expected = format!(
                "{{\"id\":{},\"outcome\":{},\"trace\":{},\"stdout\":{}}}",
                json_string(&case.id),
                case.outcome,
                case.trace,
                case.stdout
            );
            assert_eq!(local, expected, "{}: {}", case.id, case.source);
            assert_eq!(oracle, expected, "{}: {}", reference.identity, case.id);
        }
        eprintln!(
            "Matched {} sequence outcomes, effects and stdout against {}",
            cases.len(),
            reference.identity
        );
    }
}

#[test]
fn compiled_sequences_preserve_values_effects_and_failures() {
    let cases = cases()
        .into_iter()
        .filter(|case| case.id.starts_with("sequences-"))
        .collect::<Vec<_>>();
    let actual = compiled_cases(&cases);
    let mut failures = Vec::new();
    for (case, actual) in cases.iter().zip(outcomes(&actual, &cases).unwrap()) {
        let expected = format!(
            "{{\"id\":{},\"outcome\":{},\"trace\":{},\"stdout\":{}}}",
            json_string(&case.id),
            case.outcome,
            case.trace,
            case.stdout
        );
        if actual != expected {
            failures.push(format!(
                "{}: {}\nexpected {}\nactual {}",
                case.id, case.source, expected, actual
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
