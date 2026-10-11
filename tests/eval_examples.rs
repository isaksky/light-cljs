//! Compiled JavaScript examples adapted from the historical SCI fixtures.
//! Number division, :default catches and :cljs conditionals are explicit adaptations.
//! Complete examples run from an isolated directory with no ambient toolchain.
use std::{
    fs,
    path::PathBuf,
    process::{Command, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
};
#[path = "support/sci_protocol.rs"]
#[allow(dead_code)]
mod protocol;
const EXAMPLES: [(&str, &str, &str); 6] = [
    (
        "demo",
        include_str!("../examples/native/demo.cljs"),
        "{:squares [1 4 9 16 25], :sum 15}\n",
    ),
    (
        "data_processing",
        include_str!("../examples/native/data_processing.cljs"),
        "[20 7 27]\n",
    ),
    (
        "custom_macro",
        include_str!("../examples/native/custom_macro.cljs"),
        "[\"A-17\" 84 1]\n",
    ),
    (
        "lazy_pipeline",
        include_str!("../examples/native/lazy_pipeline.cljs"),
        "[[1 9 25 49] [1 2 3 4 5 6 7] [1 9 25 49]]\n",
    ),
    (
        "stateful_callbacks",
        include_str!("../examples/native/stateful_callbacks.cljs"),
        "[20 [[:audit 10 15 true] [:audit 15 18 true] [:audit 18 20 true]]]\n",
    ),
    (
        "recoverable_exceptions",
        include_str!("../examples/native/recoverable_exceptions.cljs"),
        "[[{:price 4} {:rejected 0} {:price 5}] [5 0 4]]\n",
    ),
];
struct Directory(PathBuf);
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn every_compiled_example_runs_fresh_without_java_node_or_project_files() {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let cases = protocol::cases(include_str!("fixtures/sci-outcomes.tsv")).unwrap();
    let sources = include_str!("reference/sci/load-sources.tsv");
    for (name, source, result) in EXAMPLES {
        let id = format!("sci-example-{}", name.replace('_', "-"));
        let case = cases.iter().find(|c| c.id == id).unwrap();
        let dir = Directory(std::env::temp_dir().join(format!(
            "light-example-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )));
        fs::create_dir(&dir.0).unwrap();
        let binary = dir.0.join("light-cljs");
        fs::copy(env!("CARGO_BIN_EXE_light-cljs"), &binary).unwrap();
        fs::write(dir.0.join("main.cljs"), source).unwrap();
        for (namespace, path, text) in [
            (
                "math-helpers",
                "math_helpers.cljc",
                include_str!("../examples/native/math_helpers.cljc"),
            ),
            (
                "reports.stats",
                "reports/stats.cljc",
                include_str!("../examples/native/reports/stats.cljc"),
            ),
        ] {
            let line = sources
                .lines()
                .find(|l| l.starts_with(&format!("{namespace}\t")))
                .unwrap();
            let fields: Vec<_> = line.split('\t').collect();
            assert_eq!(fields[1], path);
            assert_eq!(
                fields[2].replace(
                    ":cljs \"JavaScript\"",
                    ":cljs \"Hello from compiled JavaScript\""
                ),
                text.lines().collect::<Vec<_>>().join(" "),
                "loader drift {namespace}"
            );
            let file = dir.0.join(path);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, text).unwrap();
        }
        let out_path = dir.0.join("stdout");
        let err_path = dir.0.join("stderr");
        let mut child = Command::new(binary)
            .env_clear()
            .env("PATH", "")
            .current_dir(&dir.0)
            .args([
                "--eval",
                "--source-root",
                ".",
                "--poll-budget",
                "2000000",
                "--output-limit",
                "10000",
                "main.cljs",
            ])
            .stdin(Stdio::null())
            .stdout(fs::File::create(&out_path).unwrap())
            .stderr(fs::File::create(&err_path).unwrap())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if Instant::now() >= deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!(
                    "example {name} timed out; stdout={:?}; stderr={:?}",
                    fs::read_to_string(out_path),
                    fs::read_to_string(err_path)
                );
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        let stdout = fs::read_to_string(out_path).unwrap();
        let stderr = fs::read_to_string(err_path).unwrap();
        assert!(
            status.success(),
            "{name} crashed/failed: {status}: {stderr}; stdout={stdout}"
        );
        assert_eq!(
            stderr.encode_utf16().collect::<Vec<_>>(),
            case.stderr,
            "{name}"
        );
        assert_eq!(
            stdout,
            format!(
                "{}{result}",
                String::from_utf16(&case.stdout)
                    .unwrap()
                    .replace("Hello from native Rust", "Hello from compiled JavaScript")
            ),
            "{name}"
        );
    }
}
