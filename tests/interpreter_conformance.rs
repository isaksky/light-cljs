//! Execute every pinned fixture. Differences are assertions, never skipped rows.
#[path = "support/sci_native.rs"]
mod native;
#[path = "support/sci_protocol.rs"]
#[allow(dead_code)]
mod protocol;
use std::collections::HashMap;

#[test]
fn complete_corpus_has_an_executable_disposition() {
    let cases = protocol::cases(include_str!("fixtures/sci-outcomes.tsv")).unwrap();
    let differences = include_str!("reference/sci/native-dispositions.tsv");
    let mut reasons = HashMap::new();
    let rows: String = differences
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
        .map(|line| {
            let (row, reason) = line.rsplit_once('\t').unwrap();
            let id = row.split('\t').next().unwrap();
            assert!(!reason.is_empty());
            assert!(
                reasons.insert(id, reason).is_none(),
                "duplicate difference {id}"
            );
            format!("{row}\n")
        })
        .collect();
    let mut overrides: HashMap<_, _> = protocol::cases(&rows)
        .unwrap()
        .into_iter()
        .map(|c| (c.id, c))
        .collect();
    let mut failures = vec![];
    for case in &cases {
        let expected = if let Some(native) = overrides.remove(case.id) {
            assert_eq!(
                (native.source, native.profile),
                (case.source, case.profile),
                "stale difference {}",
                case.id
            );
            assert_ne!(
                native::Observation::expected(&native),
                native::Observation::expected(case),
                "difference {} now agrees: remove disposition",
                case.id
            );
            native::Observation::expected(&native)
        } else {
            native::Observation::expected(case)
        };
        let profile = if case.id.starts_with("sci-context-reader-tag-syntax-error-") {
            "reader-tags"
        } else {
            case.profile
        };
        let actual = native::run(case.source, profile);
        if actual.as_ref().ok() != Some(&expected) {
            failures.push(format!(
                "{}: {}\nactual {actual:?}\nexpected {expected:?}\ndisposition {:?}",
                case.id,
                case.source,
                reasons.get(case.id)
            ));
        }
    }
    assert!(overrides.is_empty(), "orphaned differences: {overrides:?}");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
