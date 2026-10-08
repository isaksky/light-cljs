use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

use light_cljs::compile;

#[derive(Clone, Debug)]
struct Case {
    id: &'static str,
    source: &'static str,
    outcome: String,
    trace: &'static str,
    stdout: &'static str,
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
                id,
                source,
                outcome: format!("{{\"status\":\"returned\",\"value\":{value}}}"),
                trace,
                stdout: "\"\"",
            }
        })
        .chain(
            rows(include_str!("fixtures/execution-outcomes.tsv")).map(|row| {
                let [id, source, outcome, trace, stdout]: [&str; 5] = row.try_into().unwrap();
                Case {
                    id,
                    source,
                    outcome: outcome.into(),
                    trace,
                    stdout,
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
    let mut script = String::from("globalThis.lightCases = [\n");
    for case in cases() {
        let source = format!("(fn* [observe] {})", case.source);
        script.push_str(
            &compile(&source)
                .unwrap_or_else(|error| panic!("{}: {}: {error}", case.id, case.source)),
        );
        script.push_str(",\n");
    }
    script.push_str("];\nglobalThis.lightCaseMetadata = [\n");
    for case in cases() {
        script.push_str(&format!(
            "[{},{}],\n",
            json_string(case.id),
            json_string(case.source)
        ));
    }
    script.push_str("];\n");
    script.push_str(include_str!("support/execution.js"));
    node(&script)
}

fn outcomes<'a>(output: &'a str, cases: &[Case]) -> Result<Vec<&'a str>, String> {
    let lines: Vec<_> = output.lines().collect();
    for (index, case) in cases.iter().enumerate() {
        let prefix = format!("{{\"id\":{},\"outcome\":", json_string(case.id));
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
                json_string(case.id),
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
        id: "silent-exit",
        source: "(js/process.exit 0)",
        outcome: String::new(),
        trace: "[]",
        stdout: "\"\"",
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

fn reference_build(reference: &Reference, label: &str, args: &[&str]) -> TempDir {
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
    let mut command = Command::new(std::env::var_os("CLOJURE").unwrap_or_else(|| "clojure".into()));
    command
        .current_dir(&root)
        .args([
            "-Srepro",
            "-Sdeps",
            &format!("{{:deps {{org.clojure/clojurescript {dependency}}}}}"),
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
        dir.0.join("reference.js").is_file(),
        "reference compiler produced no launcher"
    );
    eprintln!("{}", settings.trim());
    dir
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
            let dir = reference_build(reference, label, args);
            fs::write(dir.0.join("node-version.txt"), &node_version).unwrap();
            let script = reference_driver(&dir) + driver;
            fs::write(dir.0.join("driver.js"), &script).unwrap();
            let output = node(&script);
            fs::write(dir.0.join("results.txt"), &output).unwrap();
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
    if results.len() == 2 {
        assert_eq!(
            results[0].output, results[1].output,
            "UPSTREAM DISCREPANCY: release versus checkout for {label}; both results retained independently of light-cljs"
        );
        eprintln!("Release and checkout agree for {label}");
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
                actual, reference,
                "{}: {}: {}",
                result.identity, case.id, case.source
            );
        }
        eprintln!(
            "Matched {} program outcomes, traces, and stdout against {}",
            cases.len(),
            result.identity
        );
    }
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
