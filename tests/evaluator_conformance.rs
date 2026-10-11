//! Whole-corpus accounting for the compiler-backed evaluator migration.
#[path = "support/sci_evaluator.rs"]
mod evaluator;
#[path = "support/sci_protocol.rs"]
#[allow(dead_code)]
mod protocol;
use std::collections::{BTreeMap, HashMap, HashSet};

const CORPUS: &str = include_str!("fixtures/sci-outcomes.tsv");

fn adapted_source(source: &str) -> String {
    let mut adapted = source
        .replace("(catch Exception ", "(catch :default ")
        .replace("(catch Throwable ", "(catch :default ")
        .replace("(catch clojure.lang.ExceptionInfo ", "(catch :default ");
    // SCI's fixture environment preinstalls these namespaces. Compiler sessions
    // use the ordinary explicit loader; preserve that setup as visible source.
    for library in [
        "clojure.string",
        "clojure.set",
        "clojure.walk",
        "clojure.edn",
    ] {
        if source.contains(&format!("{library}/")) {
            adapted = format!("(require '{library}) {adapted}");
        }
    }
    adapted
}

#[test]
#[ignore = "explicit migration audit; prints observations, never updates expectations"]
fn audit_historical_corpus_with_compiler_evaluator() {
    let cases = protocol::cases(CORPUS).unwrap();
    let mut report = String::new();
    for case in cases {
        let source = adapted_source(case.source);
        let actual = evaluator::run(case.id, &source, case.profile)
            .unwrap_or_else(|error| panic!("{}: {error}", case.id));
        let compatible = evaluator::Observation::portable(&case).as_ref().ok() == Some(&actual);
        let status = if compatible {
            if source == case.source {
                "portable"
            } else {
                "adapted"
            }
        } else {
            "gap"
        };
        report.push_str(&format!(
            "{}\t{status}\t{source}\t{}\n",
            case.id,
            actual.fields()
        ));
    }
    let path = std::env::temp_dir().join("light-cljs-evaluator-corpus-audit.tsv");
    std::fs::write(&path, report).unwrap();
    println!("Audit written to {}", path.display());
}

#[test]
#[ignore = "explicit supplemental library migration audit; never updates expectations"]
fn audit_explicit_library_import_adaptations() {
    let mut report = String::new();
    for case in protocol::cases(CORPUS).unwrap() {
        let source = adapted_source(case.source);
        if ![
            "clojure.string",
            "clojure.set",
            "clojure.walk",
            "clojure.edn",
        ]
        .iter()
        .any(|library| case.source.contains(&format!("{library}/")))
        {
            continue;
        }
        let actual = evaluator::run(case.id, &source, case.profile).unwrap();
        let status = if evaluator::Observation::portable(&case).as_ref().ok() == Some(&actual) {
            if source == case.source {
                "portable"
            } else {
                "adapted"
            }
        } else {
            "gap"
        };
        report.push_str(&format!(
            "{}\t{status}\t{source}\t{}\n",
            case.id,
            actual.fields()
        ));
    }
    let path = std::env::temp_dir().join("light-cljs-evaluator-library-audit.tsv");
    std::fs::write(&path, report).unwrap();
    println!("Supplemental library audit written to {}", path.display());
}

// Filled from an explicitly reviewed audit. These rows retain the original
// seven SCI fields verbatim, then status, evaluator source and observation.
const ACCOUNTING: &str = include_str!("reference/sci/evaluator-dispositions.tsv");

#[test]
fn every_historical_sci_case_has_an_executable_evaluator_disposition() {
    let cases = protocol::cases(CORPUS).unwrap();
    let source_rows: HashMap<_, _> = CORPUS
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .map(|line| (line.split('\t').next().unwrap(), line))
        .collect();
    let mut dispositions = HashMap::new();
    for line in ACCOUNTING
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
    {
        let fields: Vec<_> = line.split('\t').collect();
        assert_eq!(fields.len(), 14, "accounting field count: {}", fields[0]);
        assert!(
            dispositions.insert(fields[0], fields).is_none(),
            "duplicate disposition"
        );
    }
    assert_eq!(
        dispositions.len(),
        cases.len(),
        "every historical row needs explicit accounting"
    );
    let mut counts = BTreeMap::<&str, usize>::new();
    let mut matching_families = BTreeMap::<String, usize>::new();
    let mut failures = vec![];
    let mut provenance_ids = HashSet::new();
    for line in include_str!("reference/fixture-origins.tsv")
        .lines()
        .filter(|line| line.starts_with("tests/fixtures/sci-outcomes.tsv\t"))
    {
        provenance_ids.extend(line.split('\t').nth(1).unwrap().split(','));
    }
    for case in &cases {
        assert!(
            provenance_ids.contains(case.id),
            "missing inherited SCI provenance: {}",
            case.id
        );
        let row = dispositions.remove(case.id).unwrap();
        assert_eq!(
            row[..7].join("\t"),
            source_rows[case.id],
            "stale SCI observation {}",
            case.id
        );
        let status = row[7];
        assert!(
            matches!(status, "portable" | "adapted" | "gap" | "retired"),
            "unknown disposition {status}"
        );
        assert!(!row[13].is_empty(), "missing reason {}", case.id);
        assert!(
            row[13].contains("owner=light-cljs-")
                && row[13].contains("kind=")
                && row[13].contains("family="),
            "missing explicit ownership/kind/family {}",
            case.id
        );
        *counts.entry(status).or_default() += 1;
        let actual = evaluator::run(case.id, row[8], case.profile);
        let expected = row[9..13].join("\t");
        if actual
            .as_ref()
            .map(evaluator::Observation::fields)
            .as_ref()
            .ok()
            != Some(&expected)
        {
            let actual_text = match &actual {
                Ok(observation) => observation.fields(),
                Err(error) => error.clone(),
            };
            let brief = |text: &str| text.chars().take(1_200).collect::<String>();
            failures.push(format!(
                "{} [{status}]: {}\nactual {}\nexpected {}\n{}",
                case.id,
                row[8],
                brief(&actual_text),
                brief(&expected),
                row[13]
            ));
        }
        if status == "portable" || status == "adapted" {
            let family = case.id.split('-').take(2).collect::<Vec<_>>().join("-");
            *matching_families.entry(family).or_default() += 1;
            let projected = evaluator::Observation::portable(case)
                .unwrap_or_else(|error| panic!("{} is not portable: {error}", case.id));
            assert_eq!(
                expected,
                projected.fields(),
                "passing disposition cannot replace SCI expectations: {}",
                case.id
            );
            assert_eq!(
                status == "portable",
                row[8] == case.source,
                "source adaptation classification {}",
                case.id
            );
            if status == "adapted" {
                assert_eq!(
                    row[8],
                    adapted_source(case.source),
                    "unreviewed source substitution {}",
                    case.id
                );
            }
        } else {
            assert_ne!(
                evaluator::Observation::portable(case)
                    .ok()
                    .map(|o| o.fields()),
                Some(expected),
                "{} now agrees with the SCI projection: migrate its disposition",
                case.id
            );
        }
    }
    assert!(dispositions.is_empty(), "orphaned evaluator dispositions");
    assert!(
        counts.get("portable").copied().unwrap_or(0) >= 1_000,
        "broad portable corpus required: {counts:?}"
    );
    for family in [
        "sci-lazy",
        "sci-xf",
        "sci-control",
        "sci-binding",
        "sci-coll",
        "sci-seq",
        "sci-state",
        "sci-string",
        "sci-data",
    ] {
        assert!(
            matching_families.get(family).copied().unwrap_or(0) >= 30,
            "retained meaningful {family} observations cannot disappear from matching coverage"
        );
    }
    println!("Evaluator accounting (gaps/retirements are not conformance passes): {counts:?}");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn cljs_projection_keeps_numeric_bits_and_explicitly_rejects_jvm_only_numbers() {
    use protocol::Value;
    assert_eq!(
        evaluator::portable(&Value::Int(42)).unwrap(),
        Value::Float(42f64.to_bits())
    );
    assert_eq!(
        evaluator::portable(&Value::Float((-0f64).to_bits())).unwrap(),
        Value::Float((-0f64).to_bits())
    );
    for value in [
        Value::Int(i64::MAX),
        Value::Ratio(1, 2),
        Value::BigInt("1".into()),
        Value::Decimal("120".into(), 2),
    ] {
        assert!(evaluator::portable(&value).is_err());
    }
    assert_eq!(
        evaluator::portable(&Value::Char(0xd800)).unwrap(),
        Value::String(vec![0xd800])
    );
    assert_eq!(
        evaluator::portable(&Value::Seq(vec![])).unwrap(),
        Value::List(vec![])
    );
}
