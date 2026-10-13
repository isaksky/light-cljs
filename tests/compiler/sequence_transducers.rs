//! Lazy transducing sequence observations measured against both pinned CLJS references.
use light_cljs::{
    compile,
    evaluator::{Evaluator, EvaluatorOptions},
};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

const FIXTURES: &str = include_str!("../fixtures/compiler-sequence-transducers.tsv");
const REVISION: &str = "a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e";

#[derive(Debug)]
struct Case<'a> {
    id: &'a str,
    source: &'a str,
    expected: &'a str,
    origin: &'a str,
    release: &'a str,
    local: &'a str,
}

fn cases() -> Vec<Case<'static>> {
    FIXTURES
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| {
            let fields: Vec<_> = line.split('\t').collect();
            assert_eq!(fields.len(), 6, "invalid transducer fixture: {line}");
            Case {
                id: fields[0],
                source: fields[1],
                expected: fields[2],
                origin: fields[3],
                release: fields[4],
                local: fields[5],
            }
        })
        .collect()
}

fn node(script: &str) -> String {
    let mut child = Command::new(std::env::var_os("NODE").unwrap_or_else(|| "node".into()))
        .args(["--max-old-space-size=96", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(script.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn lazy_sequence_values_effects_and_errors_match_in_quickjs_and_node() {
    let cases = cases();
    let mut failures = Vec::new();
    for group in cases.chunks(8) {
        let mut script = String::new();
        let mut compiled = Vec::new();
        for case in group {
            let expected = if case.local == "-" {
                case.expected
            } else {
                case.local
            };
            let javascript =
                compile(case.source).unwrap_or_else(|error| panic!("{}: {error}", case.id));
            // Each case receives its own runtime and reducer instances in both engines.
            let evaluator = Evaluator::new(EvaluatorOptions {
                timeout: Duration::from_secs(60),
                ..EvaluatorOptions::default()
            })
            .unwrap();
            let actual = evaluator
                .evaluate_js(&format!("JSON.stringify({javascript})"))
                .and_then(|value| evaluator.string(value));
            if actual.as_deref() != Ok(expected) {
                failures.push(format!(
                    "QuickJS {}: expected {}, got {:?}",
                    case.id, expected, actual
                ));
            }
            compiled.push(javascript);
        }
        for javascript in &compiled {
            script.push_str(&format!("console.log(JSON.stringify({javascript}));\n"));
        }
        let output = node(&script);
        let lines: Vec<_> = output.lines().collect();
        assert_eq!(
            lines.len(),
            group.len(),
            "missing or extra Node observations"
        );
        for (case, actual) in group.iter().zip(lines) {
            let expected = if case.local == "-" {
                case.expected
            } else {
                case.local
            };
            if actual != expected {
                failures.push(format!(
                    "Node {}: expected {}, got {actual}",
                    case.id, expected
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
#[ignore = "requires Clojure CLI, Java, Node, and both pinned CLJS references"]
fn lazy_sequence_fixtures_match_pinned_release_and_checkout() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let checkout = std::env::var_os("LIGHT_CLJS_REFERENCE_CHECKOUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("research/clojurescript"));
    let cases = cases();
    for (kind, identity, dependency, checkout_arg) in [
        (
            "release",
            "1.12.134",
            "{:mvn/version \"1.12.134\"}".to_string(),
            "-".to_string(),
        ),
        (
            "checkout",
            REVISION,
            format!("{{:local/root {:?}}}", checkout),
            checkout.to_string_lossy().into_owned(),
        ),
    ] {
        let output_dir = std::env::temp_dir().join(format!(
            "light-cljs-sequence-transducers-{kind}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&output_dir).unwrap();
        let mut child =
            Command::new(std::env::var_os("CLOJURE").unwrap_or_else(|| "clojure".into()))
                .current_dir(&root)
                .args([
                    "-Srepro",
                    "-Sdeps",
                    &format!("{{:deps {{org.clojure/clojurescript {dependency}}}}}"),
                    "-M",
                    "tests/reference/execution.clj",
                    kind,
                    identity,
                    &checkout_arg,
                ])
                .arg(&output_dir)
                .arg("cases")
                .arg(root.join("tests/fixtures/compiler-sequence-transducers.tsv"))
                .stdin(Stdio::null())
                .stdout(fs::File::create(output_dir.join("build.stdout")).unwrap())
                .stderr(fs::File::create(output_dir.join("build.stderr")).unwrap())
                .spawn()
                .unwrap();
        let start = Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if start.elapsed() > Duration::from_secs(240) {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!(
                    "{kind} reference timed out; artifacts at {}",
                    output_dir.display()
                );
            }
            std::thread::sleep(Duration::from_millis(25));
        };
        let settings = fs::read_to_string(output_dir.join("build.stdout")).unwrap();
        assert!(
            status.success(),
            "{kind}: {}\nartifacts: {}",
            fs::read_to_string(output_dir.join("build.stderr")).unwrap(),
            output_dir.display()
        );
        assert!(
            settings.contains(&format!("REFERENCE IDENTITY {kind} {identity}")),
            "{settings}"
        );
        assert!(
            settings.contains(
                "REFERENCE SETTINGS {:optimizations :none, :static-fns false, :def-emits-var false}"
            ),
            "{settings}"
        );
        let output = node(&format!(
            "process.chdir({:?}); require({:?}); for (const f of globalThis.lightCases) console.log(JSON.stringify(f(() => {{ throw new Error('unexpected outer observer'); }})));",
            output_dir,
            output_dir.join("reference.js")
        ));
        let lines: Vec<_> = output.lines().collect();
        assert_eq!(lines.len(), cases.len(), "{kind}");
        for (case, actual) in cases.iter().zip(lines) {
            let expected = if kind == "release" && case.release != "-" {
                case.release
            } else {
                case.expected
            };
            assert_eq!(actual, expected, "{kind}: {}", case.id);
        }
        eprintln!(
            "Matched {} lazy sequence observations against {kind} {identity}",
            cases.len()
        );
        fs::remove_dir_all(output_dir).unwrap();
    }
}

#[test]
fn sequence_fixture_provenance_retains_complete_body_hashes_and_literal_traces() {
    let cases = cases();
    let mut ids = std::collections::HashSet::new();
    for case in &cases {
        assert!(ids.insert(case.id), "duplicate {}", case.id);
        assert_ne!(case.expected, "-", "unmeasured {}", case.id);
        assert_eq!(
            case.release, "-",
            "unexpected version difference {}",
            case.id
        );
        assert_eq!(case.local, "-", "unexpected local difference {}", case.id);
        assert!(!case.origin.is_empty());
    }
    for (name, hash, assertions) in [
        (
            "test-cljs-2028",
            "57c016e5a7eb7c68dab2311bca66203453020972aae4665703e95569f9c2625d",
            3,
        ),
        (
            "test-ISequential-indexOf",
            "c472d7a9bb8ba310a8e01211eb4543f6b149e933c4303b69b3c3af42217b0eee",
            14,
        ),
        (
            "test-ISequential-lastIndexOf",
            "9f102d1380757cc4b6bd3f2a17ee5e70b7b291068fda21bd351c1c0f92faf0b5",
            17,
        ),
    ] {
        let case = cases
            .iter()
            .find(|c| c.id == format!("upstream-{name}"))
            .unwrap();
        assert!(case.origin.ends_with(&format!(";sha256={hash}")));
        assert_eq!(
            case.source.matches("(assert ").count(),
            assertions,
            "{name}"
        );
    }
    assert_eq!(cases.len(), 30);
}
