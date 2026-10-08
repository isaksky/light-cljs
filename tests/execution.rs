use std::{
    fs,
    io::{Read, Write},
    path::PathBuf,
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
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

fn reference_build(label: &str, args: &[&str]) -> TempDir {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let dir = TempDir(std::env::temp_dir().join(format!(
        "light-cljs-reference-{label}-{}",
        std::process::id()
    )));
    fs::create_dir_all(&dir.0).unwrap();
    let mut command = Command::new(std::env::var_os("CLOJURE").unwrap_or_else(|| "clojure".into()));
    command
        .current_dir(&root)
        .args([
            "-Srepro",
            "-Sdeps",
            "{:deps {org.clojure/clojurescript {:mvn/version \"1.12.134\"}}}",
            "-M",
            "tests/reference/execution.clj",
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
    let output = run(command, Vec::new(), Duration::from_secs(timeout));
    let settings = String::from_utf8(output.stdout).unwrap();
    assert!(settings.contains("REFERENCE SETTINGS 1.12.134 {:optimizations :none, :static-fns false, :def-emits-var false}"), "missing verified reference settings: {settings}");
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

#[test]
#[ignore = "requires Clojure CLI, Java, Node, and the pinned ClojureScript Maven dependencies"]
fn programs_match_standard_clojurescript_results_and_effects() {
    let dir = reference_build("cases", &[]);
    let reference = node(&(reference_driver(&dir) + include_str!("support/execution.js")));
    let actual = compiled_results();
    assert_eq!(reference.lines().count(), cases().len());
    assert_eq!(actual.lines().count(), cases().len());
    for (([name, source, _, _], actual), reference) in cases()
        .into_iter()
        .zip(actual.lines())
        .zip(reference.lines())
    {
        assert_eq!(actual, reference, "{name}: {source}");
    }
    eprintln!(
        "Matched {} programs and effect traces against Maven ClojureScript 1.12.134",
        cases().len()
    );
}

#[test]
#[ignore = "requires Clojure CLI, Java, Node, and the pinned ClojureScript Maven dependencies"]
fn namespace_program_matches_standard_clojurescript_stdout() {
    let source_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/namespaces");
    let dir = reference_build(
        "namespaces",
        &["program", source_root.to_str().unwrap(), "demo.main"],
    );
    let reference = node(&reference_driver(&dir));
    let js = light_cljs::compiler::compile_namespace("demo.main", &[source_root]).unwrap();
    let actual = node(&js);
    assert_eq!(reference, "4\n", "reference acceptance output");
    assert_eq!(actual, reference, "two-namespace acceptance output");
    eprintln!("Both compilers produced exactly stdout 4\\n for demo.main");
}

#[test]
#[ignore = "requires Clojure CLI, Java, Node, and the pinned ClojureScript Maven dependencies"]
fn dependency_initialization_matches_standard_clojurescript() {
    let source_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/namespaces");
    let dir = reference_build(
        "dependency-order",
        &["program", source_root.to_str().unwrap(), "order.main"],
    );
    let reference = node(&reference_driver(&dir));
    let js = light_cljs::compiler::compile_namespace("order.main", &[source_root]).unwrap();
    let actual = node(&js);
    assert_eq!(reference, "shared\nleft\nright\nmain\n10\n20\n7\n7\n40\n");
    assert_eq!(
        actual, reference,
        "diamond dependency order and global values"
    );
    eprintln!("Both compilers initialized the shared dependency once, in matching order");
}
