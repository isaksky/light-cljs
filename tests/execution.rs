use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};

use light_cljs::compile;

fn cases() -> Vec<[&'static str; 4]> {
    include_str!("fixtures/execution.tsv")
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .map(|line| line.split('\t').collect::<Vec<_>>().try_into().unwrap())
        .collect()
}

fn node(script: &str) -> String {
    let mut child = Command::new(std::env::var_os("NODE").unwrap_or_else(|| "node".into()))
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("execution tests require Node.js on PATH (or NODE=/path/to/node)");
    // Write concurrently so large input and Node's output cannot deadlock pipes.
    let mut input = child.stdin.take().unwrap();
    let script = script.to_owned();
    let writer = std::thread::spawn(move || input.write_all(script.as_bytes()));
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
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
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
#[ignore = "requires Clojure CLI, Java, Node, and the pinned ClojureScript Maven dependencies"]
fn programs_match_standard_clojurescript_results_and_effects() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let dir =
        TempDir(std::env::temp_dir().join(format!("light-cljs-reference-{}", std::process::id())));
    fs::create_dir_all(&dir.0).unwrap();
    let output = Command::new("clojure")
        .current_dir(&root)
        .args([
            "-Sdeps",
            "{:deps {org.clojure/clojurescript {:mvn/version \"1.12.134\"}}}",
            "-M",
            "tests/reference/execution.clj",
        ])
        .arg(&dir.0)
        .output()
        .expect("Clojure CLI is required for the differential test");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    // The reference Node launcher resolves its asset path from the process cwd.
    let driver = format!(
        "process.chdir({:?});\nrequire({:?});\n{}",
        dir.0.to_str().unwrap(),
        dir.0.join("reference.js").to_str().unwrap(),
        include_str!("support/execution.js")
    );
    let reference = node(&driver);
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
}
