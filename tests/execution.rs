use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use light_cljs::compile;

fn cases() -> Vec<[&'static str; 4]> {
    include_str!("fixtures/execution.tsv")
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .map(|line| line.split('\t').collect::<Vec<_>>().try_into().unwrap())
        .collect()
}

fn run(mut command: Command, input: Vec<u8>, timeout: Duration) -> Output {
    let description = format!("{command:?}");
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|error| panic!("required tool unavailable: {description}: {error}"));
    // Drain both output pipes concurrently so a verbose compiler cannot deadlock.
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || stdin.write_all(&input));
    let mut stdout = child.stdout.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout.read_to_end(&mut bytes).unwrap();
        bytes
    });
    let mut stderr = child.stderr.take().unwrap();
    let errors = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stderr.read_to_end(&mut bytes).unwrap();
        bytes
    });
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
        std::thread::sleep(Duration::from_millis(25));
    };
    // A failed launcher can leave descendants holding inherited pipes. Fail
    // before joining the readers so that those handles cannot defeat the bound.
    assert!(
        !timed_out,
        "required tool timed out after {timeout:?}: {description}"
    );
    let output = Output {
        status,
        stdout: reader.join().unwrap(),
        stderr: errors.join().unwrap(),
    };
    let written = writer.join().unwrap();
    assert!(
        output.status.success(),
        "required tool failed: {description}\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    written.unwrap();
    output
}

fn node(script: &str) -> String {
    let mut command = Command::new(std::env::var_os("NODE").unwrap_or_else(|| "node".into()));
    command.arg("-");
    let output = run(command, script.as_bytes().to_vec(), Duration::from_secs(30));
    String::from_utf8(output.stdout).unwrap()
}

fn compiled_results() -> String {
    let mut script = String::from("globalThis.lightCases = [\n");
    for [name, source, _, _] in cases() {
        let source = format!("(fn* [observe] {source})");
        script.push_str(&compile(&source).unwrap_or_else(|error| panic!("{name}: {error}")));
        script.push_str(",\n");
    }
    script.push_str("];\n");
    script.push_str(include_str!("support/execution.js"));
    node(&script)
}

#[test]
fn tiny_programs_execute_with_expected_values_and_effects() {
    let output = compiled_results();
    let results: Vec<_> = output.lines().collect();
    assert_eq!(results.len(), cases().len());
    for ([name, source, value, trace], actual) in cases().into_iter().zip(results) {
        assert_eq!(
            actual,
            format!("{{\"value\":{value},\"trace\":{trace}}}"),
            "{name}: {source}"
        );
    }
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
impl Drop for TempDir {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!(
                "Reference failure artifacts retained at {}",
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
    let dir = TempDir(std::env::temp_dir().join(format!(
        "light-cljs-reference-{label}-{}-{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
    )));
    fs::create_dir(&dir.0).unwrap();
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
        assert_eq!(reference.lines().count(), cases().len());
        assert_eq!(actual.lines().count(), cases().len());
        for (([name, source, _, _], actual), reference) in cases()
            .into_iter()
            .zip(actual.lines())
            .zip(reference.lines())
        {
            assert_eq!(actual, reference, "{}: {name}: {source}", result.identity);
        }
        eprintln!(
            "Matched {} programs and effect traces against {}",
            cases().len(),
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
