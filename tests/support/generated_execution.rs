//! Runs the v1 programs of generated.rs under their four metamorphic variants
//! through the shared Node protocol (support/execution.js), and optionally
//! against standard ClojureScript.
use super::generated::{SEEDS, VARIANTS, program, seeds};
use super::*;

fn generated_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for seed in seeds() {
        eprintln!("GENERATED v1 seed {seed}");
        let form = program(seed);
        assert!(
            form.nodes() <= 100 && form.depth() <= 12,
            "seed {seed}: generator bounds"
        );
        let original = form.source(false);
        // The identity name is outside both alpha-renaming alphabets and is
        // scoped only after its initializer. Each expression executes once.
        let sources = [
            original.clone(),
            form.source(true),
            format!("(let* [identityFresh {original}] identityFresh)"),
            format!("(do {original})"),
        ];
        for (variant, source) in VARIANTS.iter().zip(sources) {
            assert!(source.len() <= 8192, "seed {seed}: source bound");
            cases.push(Case {
                id: format!("generated-v1-{seed}-{variant}"),
                source,
                outcome: String::new(),
                trace: String::new(),
                stdout: String::new(),
            });
        }
    }
    cases
}

fn input_cases() -> Vec<Case> {
    if let Some(path) = std::env::var_os("LIGHT_CLJS_GENERATED_REPLAY_TSV") {
        assert!(
            std::env::var_os("LIGHT_CLJS_GENERATED_SEED").is_none()
                && std::env::var_os("LIGHT_CLJS_GENERATED_COUNT").is_none(),
            "select seeds or a replay TSV, not both"
        );
        let text = fs::read_to_string(&path).expect("required generated replay TSV unavailable");
        return replay_cases(&text);
    }
    generated_cases()
}

fn replay_cases(text: &str) -> Vec<Case> {
    assert!(text.len() <= 512 * 1024, "replay source exceeds bound");
    let cases: Vec<_> = text
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| {
            let (id, source) = line
                .split_once('\t')
                .expect("replay rows require ID and source");
            assert!(
                !source.contains('\t') && source.len() <= 8192,
                "invalid replay source"
            );
            Case {
                id: id.into(),
                source: source.into(),
                outcome: String::new(),
                trace: String::new(),
                stdout: String::new(),
            }
        })
        .collect();
    assert!(
        !cases.is_empty() && cases.len() <= 48 && cases.len() % 4 == 0,
        "replay requires 1–12 complete four-variant groups"
    );
    for group in cases.as_chunks::<4>().0 {
        let base = group[0]
            .id
            .strip_suffix("-original")
            .expect("first replay variant must be original");
        for (case, variant) in group.iter().zip(VARIANTS) {
            assert_eq!(
                case.id,
                format!("{base}-{variant}"),
                "replay variants missing or reordered"
            );
        }
    }
    cases
}

fn equivalent(output: &str, cases: &[Case]) -> Result<(), String> {
    let lines = outcomes(output, cases)?;
    let (groups, remainder) = cases.as_chunks::<4>();
    if groups.is_empty() || !remainder.is_empty() {
        return Err("expected complete four-variant groups".into());
    }
    for (group, actual) in groups.iter().zip(lines.as_chunks::<4>().0) {
        let mut baseline = None;
        for (case, line) in group.iter().zip(actual) {
            let prefix = format!("{{\"id\":{},", json_string(&case.id));
            let observable = line.strip_prefix(&prefix).unwrap();
            if !observable.starts_with("\"outcome\":{\"status\":\"returned\",") {
                return Err(format!(
                    "valid generated program did not return: {}: {}\n{line}",
                    case.id, case.source
                ));
            }
            if let Some(expected) = baseline {
                if observable != expected {
                    return Err(format!(
                        "metamorphic mismatch: {}\noriginal: {}\ncounterpart: {}\n{expected}\n{observable}",
                        case.id, group[0].source, case.source
                    ));
                }
            } else {
                baseline = Some(observable);
            }
        }
    }
    Ok(())
}

fn save_cases(cases: &[Case]) -> TempDir {
    let dir = TempDir::new("generated-v1");
    fs::write(
        dir.0.join("cases.tsv"),
        cases
            .iter()
            .map(|case| format!("{}\t{}\n", case.id, case.source))
            .collect::<String>(),
    )
    .unwrap();
    dir
}

#[test]
fn generated_programs_preserve_results_and_traces_under_transformations() {
    let cases = input_cases();
    let _artifacts = save_cases(&cases);
    let output = compiled_cases(&cases);
    equivalent(&output, &cases).unwrap_or_else(|error| panic!("{error}"));
}

#[test]
#[ignore = "requires Clojure CLI, Java, Node, and the pinned ClojureScript Maven dependencies"]
fn generated_programs_match_standard_clojurescript() {
    compare_reference_cases(&input_cases());
}

fn compare_reference_cases(cases: &[Case]) {
    let artifacts = save_cases(cases);
    let path = artifacts.0.join("cases.tsv");
    let references = reference_results(
        "generated-v1",
        &["cases", path.to_str().unwrap()],
        include_str!("execution.js"),
    );
    let actual = compiled_cases(cases);
    equivalent(&actual, cases).unwrap_or_else(|error| panic!("{error}"));
    let actual = outcomes(&actual, cases).unwrap();
    for reference in &references {
        equivalent(&reference.output, cases)
            .unwrap_or_else(|error| panic!("{}: {error}", reference.identity));
        let expected = outcomes(&reference.output, cases).unwrap();
        for ((case, actual), expected) in cases.iter().zip(&actual).zip(expected) {
            assert_eq!(
                *actual, expected,
                "{}: {}\n{}",
                reference.identity, case.id, case.source
            );
        }
        eprintln!(
            "Matched {} generated programs ({} seeds × 4 variants), outcomes and traces against {}",
            cases.len(),
            cases.len() / 4,
            reference.identity
        );
    }
}

#[test]
fn generated_comparison_rejects_changed_traces_errors_and_missing_results() {
    let cases = generated_cases();
    let cases = &cases[..4];
    let output = compiled_cases(cases);
    equivalent(&output, cases).unwrap();
    assert!(
        equivalent(
            &output.replacen("\"callee\",\"argument\"", "\"argument\",\"callee\"", 1),
            cases
        )
        .is_err()
    );
    assert!(equivalent(&output.replace("\"returned\"", "\"threw\""), cases).is_err());
    assert!(equivalent(output.lines().next().unwrap(), cases).is_err());
}

#[test]
fn generated_seed_manifest_is_stable_and_bounded() {
    let mut actual = String::new();
    for seed in SEEDS {
        let form = program(seed);
        let source = form.source(false);
        // FNV-1a identifies the exact generated source; it is not a security hash.
        let fingerprint = source.bytes().fold(14695981039346656037_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(1099511628211)
        });
        actual.push_str(&format!(
            "v1\t{seed}\t{fingerprint:016x}\t{}\t{}\n",
            form.nodes(),
            form.depth()
        ));
    }
    let expected = include_str!("../reference/generated-origins.tsv")
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| format!("{line}\n"))
        .collect::<String>();
    assert_eq!(
        actual, expected,
        "generated origin changed: bump version or deliberately refresh the seed manifest after conformance"
    );
}
