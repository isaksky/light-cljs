//! Whole-corpus accounting for the compiler-backed evaluator migration.
#[path = "../support/sci_evaluator.rs"]
mod evaluator;
use crate::protocol::{self, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

const CORPUS: &str = include_str!("../fixtures/sci-outcomes.tsv");

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
    let selected_ids = std::env::var("LIGHT_CLJS_SCI_AUDIT_IDS")
        .ok()
        .map(|ids| ids.split(',').map(str::to_owned).collect::<HashSet<_>>());
    let mut report = String::new();
    for case in cases {
        if selected_ids
            .as_ref()
            .is_some_and(|ids| !ids.contains(case.id))
        {
            continue;
        }
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
    // The report is the output of this explicit audit, so it stays: one file whose
    // fixed name each run overwrites.
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
    // Kept on purpose, as in `audit_historical_corpus_with_compiler_evaluator`.
    let path = std::env::temp_dir().join("light-cljs-evaluator-library-audit.tsv");
    std::fs::write(&path, report).unwrap();
    println!("Supplemental library audit written to {}", path.display());
}

// Filled from an explicitly reviewed audit. These rows retain the original
// seven SCI fields verbatim, then status, evaluator source and observation.
const ACCOUNTING: &str = include_str!("../reference/sci/evaluator-dispositions.tsv");

// A reviewed canonical reader difference, separate from the immutable original
// SCI row and historical migration observation. Match the entire retained body,
// including its source, expectation, effects/output and provenance rationale.
const ANONYMOUS_NONPOSITIVE_BODY: &str = concat!(
    "sci-reader-anonymous-nonpositive\tportable\t",
    "(let [f '#(%0 %-1)] [(count (second f)) (count (nth f 2))])\t",
    "(return (vector (int 0) (int 2)))\t(vector)\t(string u)\t(string u)\tportable\t",
    "(let [f '#(%0 %-1)] [(count (second f)) (count (nth f 2))])\t",
    "(return (vector (float 0000000000000000) (float 4000000000000000)))\t",
    "(vector)\t(string u)\t(string u)\t",
    "owner=light-cljs-114; kind=matching-observation; family=sci-reader; SCI observation agrees under the documented CLJS Number/character/finite-sequence projection; original source and provenance retained."
);
const ANONYMOUS_NONPOSITIVE_CANONICAL: &str = concat!(
    "(return (vector (float 4000000000000000) (float 4000000000000000)))\t",
    "(vector)\t(string u)\t(string u)"
);

fn canonical_anonymous_contract(row: &[&str]) -> Option<&'static str> {
    (row.join("\t") == ANONYMOUS_NONPOSITIVE_BODY).then_some(ANONYMOUS_NONPOSITIVE_CANONICAL)
}

fn canonical_anonymous_reconciliation(row: &[&str], observed: &str) -> Option<&'static str> {
    (canonical_anonymous_contract(row) == Some(observed)).then_some("canonical-dialect-difference")
}

#[test]
fn canonical_anonymous_accounting_requires_exact_body_and_complete_observation() {
    assert!(
        ACCOUNTING
            .lines()
            .any(|line| line == ANONYMOUS_NONPOSITIVE_BODY)
    );
    let row: Vec<_> = ANONYMOUS_NONPOSITIVE_BODY.split('\t').collect();
    assert_eq!(
        canonical_anonymous_reconciliation(&row, ANONYMOUS_NONPOSITIVE_CANONICAL),
        Some("canonical-dialect-difference")
    );
    assert_ne!(row[9..13].join("\t"), ANONYMOUS_NONPOSITIVE_CANONICAL);
    for index in [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13] {
        let mut changed = row.clone();
        changed[index] = "changed";
        assert_eq!(
            canonical_anonymous_reconciliation(&changed, ANONYMOUS_NONPOSITIVE_CANONICAL),
            None,
            "changed body column {index}"
        );
    }
    let fields: Vec<_> = ANONYMOUS_NONPOSITIVE_CANONICAL.split('\t').collect();
    for (index, value) in [
        (0, row[9]),
        (1, "(vector (int 1))"),
        (2, "(string u0078)"),
        (3, "(string u0078)"),
    ] {
        let mut changed = fields.clone();
        changed[index] = value;
        assert_eq!(
            canonical_anonymous_reconciliation(&row, &changed.join("\t")),
            None,
            "changed observation column {index}"
        );
    }
}

// Diagnostic migration receipts preserve the entire historical row. They are
// still nonpassing gaps; revised compiler attribution cannot count as parity.
const QUOTED_MACRO_DIAGNOSTICS: &str =
    include_str!("../reference/sci/quoted-macro-diagnostic-reconciliation.tsv");

fn current_quoted_macro_diagnostic(row: &[&str]) -> Option<String> {
    QUOTED_MACRO_DIAGNOSTICS
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .find_map(|line| {
            let receipt: Vec<_> = line.split('\t').collect();
            assert_eq!(receipt.len(), 19, "complete diagnostic reconciliation");
            (receipt[..14] == *row).then(|| receipt[14..18].join("\t"))
        })
}

#[test]
fn quoted_macro_diagnostic_receipts_preserve_exact_historical_gap_bodies() {
    let rows: Vec<_> = QUOTED_MACRO_DIAGNOSTICS
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .collect();
    assert_eq!(rows.len(), 6);
    let mut ids = HashSet::new();
    for receipt in rows {
        let fields: Vec<_> = receipt.split('\t').collect();
        assert_eq!(fields.len(), 19);
        assert!(ids.insert(fields[0]));
        let original = fields[..14].join("\t");
        assert!(ACCOUNTING.lines().any(|line| line == original));
        assert_eq!(fields[7], "gap");
        assert_eq!(fields[2], fields[8], "no source adaptation");
        assert!(fields[14].starts_with("(diagnostic (string u"));
        assert_eq!(fields[10..13], fields[15..18], "effects/output retained");
        assert_ne!(fields[9], fields[14], "current attribution is distinct");
        assert!(fields[18].contains("never a SCI conformance or intended-language pass"));
        let row = &fields[..14];
        assert_eq!(
            current_quoted_macro_diagnostic(row),
            Some(fields[14..18].join("\t"))
        );
        for column in 0..14 {
            let mut changed = row.to_vec();
            changed[column] = "changed";
            assert_eq!(current_quoted_macro_diagnostic(&changed), None);
        }
    }
}

// Rows whose evaluator observation ends in QuickJS stack exhaustion. How many
// re-entrant realizations fit in the 1 MiB engine stack, and whether JavaScript
// or the observe host callback reaches the limit first, depend on native frame
// sizes and so on the build profile (for example the opt-level of QuickJS). The
// rows keep a recorded observation but are compared by shape.
const STACK_DEPTH_DEPENDENT: &[&str] = &["sci-deferred-lazy-reentry"];
const STACK_EXHAUSTED: [&str; 2] = [
    "Error: Maximum call stack size exceeded",
    "Error: user/observe: Error: Maximum call stack size exceeded",
];
const REALIZING: &str = " (bool false)";

fn wire(text: &str) -> String {
    protocol::Value::String(text.encode_utf16().collect()).wire()
}

/// The number of `false` effects (each realization observes that it is not yet
/// realized before re-entering) for a stack-exhaustion diagnostic without
/// output; `None` for any other observation.
fn stack_exhaustion_depth(fields: &str) -> Option<usize> {
    let [outcome, effects, stdout, stderr] = fields.split('\t').collect::<Vec<_>>()[..] else {
        return None;
    };
    let exhausted = STACK_EXHAUSTED
        .iter()
        .any(|message| outcome == format!("(diagnostic {})", wire(message)));
    let effects = effects.strip_prefix("(vector")?.strip_suffix(')')?;
    let depth = effects.len() / REALIZING.len();
    (exhausted && effects == REALIZING.repeat(depth) && stdout == wire("") && stderr == wire(""))
        .then_some(depth)
}

/// A recorded stack-exhaustion row matches any observation of the same shape
/// that re-entered at least once, whatever its depth.
fn stack_depth_reconciliation(row: &[&str], observed: &str) -> bool {
    STACK_DEPTH_DEPENDENT.contains(&row[0])
        && stack_exhaustion_depth(&row[9..13].join("\t")).is_some()
        && stack_exhaustion_depth(observed).is_some_and(|depth| depth >= 2)
}

#[test]
fn stack_depth_accounting_requires_exhaustion_after_reentry() {
    let row = ACCOUNTING
        .lines()
        .find(|line| line.starts_with("sci-deferred-lazy-reentry\t"))
        .unwrap();
    let row: Vec<_> = row.split('\t').collect();
    let recorded = row[9..13].join("\t");
    assert!(stack_exhaustion_depth(&recorded).is_some_and(|depth| depth >= 2));
    let observation = |message: &str, effects: &str, stdout: &str| {
        format!(
            "(diagnostic {})\t(vector{effects})\t{}\t{}",
            wire(message),
            wire(stdout),
            wire("")
        )
    };
    for message in STACK_EXHAUSTED {
        for depth in [2, 16, 142, 1_000] {
            let observed = observation(message, &REALIZING.repeat(depth), "");
            assert!(stack_depth_reconciliation(&row, &observed), "{depth}");
        }
    }
    let exhausted = STACK_EXHAUSTED[0];
    for observed in [
        observation(exhausted, "", ""),
        observation(exhausted, REALIZING, ""),
        observation(exhausted, &format!("{REALIZING}{REALIZING} nil"), ""),
        observation(exhausted, &format!("{REALIZING} (bool true)"), ""),
        observation(exhausted, &REALIZING.repeat(3), "x"),
        observation("Error: boom", &REALIZING.repeat(3), ""),
        observation(
            "Error: user/observe: Maximum call stack size exceeded",
            &REALIZING.repeat(3),
            "",
        ),
        // SCI's inner first returns nil, so its realization completes.
        "(return nil)\t(vector (bool false) nil (bool true))\t(string u)\t(string u)".into(),
        "(resource-bound lazy-cycle)\t(vector (bool false) (bool false))\t(string u)\t(string u)"
            .into(),
    ] {
        assert!(!stack_depth_reconciliation(&row, &observed), "{observed}");
    }
    let observed = observation(exhausted, &REALIZING.repeat(3), "");
    let mut other = row.clone();
    other[0] = "sci-deferred-lazy-self";
    assert!(!stack_depth_reconciliation(&other, &observed));
    let mut returned = row.clone();
    returned[9] = "(return nil)";
    assert!(!stack_depth_reconciliation(&returned, &observed));
}

// An anonymous function literal names its parameters `p<n>__<id>#` and
// `rest__<id>#`. The reader draws <id> from a process-wide counter, so its value
// depends on what the process read before: plain `cargo test` runs a binary's
// tests in one process, nextest gives each test its own. Observations are
// compared with every id replaced by its rank among the ids in that observation.
// Names stay equal or distinct exactly as observed and keep their read order;
// only the process history is dropped.

/// The parameter spelling and id of an anonymous function parameter name.
fn anonymous_parameter(name: &[u16]) -> Option<(String, u64)> {
    let name = String::from_utf16(name).ok()?;
    let (parameter, id) = name.strip_suffix('#')?.rsplit_once("__")?;
    let canonical_index = |index: &str| index.parse::<i64>().is_ok_and(|n| n.to_string() == index);
    let id = id.parse::<u64>().ok().filter(|n| n.to_string() == id)?;
    (parameter == "rest" || parameter.strip_prefix('p').is_some_and(canonical_index))
        .then(|| (parameter.to_owned(), id))
}

fn map_symbols(value: &Value, f: &mut impl FnMut(&[u16]) -> Option<Vec<u16>>) -> Value {
    let mut all = |values: &[Value]| values.iter().map(|value| map_symbols(value, f)).collect();
    match value {
        Value::Symbol(None, name) => Value::Symbol(None, f(name).unwrap_or_else(|| name.clone())),
        Value::List(values) => Value::List(all(values)),
        Value::Vector(values) => Value::Vector(all(values)),
        Value::Seq(values) => Value::Seq(all(values)),
        Value::Set(values) => Value::Set(all(values)),
        Value::Map(entries) => Value::Map(
            entries
                .iter()
                .map(|(key, value)| (map_symbols(key, f), map_symbols(value, f)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// The returned value, if any, and the effects of observation fields.
fn observation_data(fields: &str) -> Option<(Option<Value>, Value)> {
    let [outcome, effects, _, _] = fields.split('\t').collect::<Vec<_>>()[..] else {
        return None;
    };
    let returned = outcome
        .strip_prefix("(return ")
        .and_then(|value| value.strip_suffix(')'));
    Some((
        returned.map(protocol::value).transpose().ok()?,
        protocol::value(effects).ok()?,
    ))
}

/// Observation fields with anonymous parameter ids ranked, in canonical order.
fn ranked_anonymous_parameters(fields: &str) -> String {
    let Some((returned, effects)) = observation_data(fields) else {
        return fields.to_owned();
    };
    let mut ids = BTreeSet::new();
    for value in returned.iter().chain([&effects]) {
        map_symbols(value, &mut |name| {
            ids.extend(anonymous_parameter(name).map(|(_, id)| id));
            None
        });
    }
    let rank: HashMap<_, _> = ids.into_iter().zip(1..).collect();
    let mut rename = |name: &[u16]| {
        let (parameter, id) = anonymous_parameter(name)?;
        Some(
            format!("{parameter}__{}#", rank[&id])
                .encode_utf16()
                .collect(),
        )
    };
    let mut fields: Vec<_> = fields.split('\t').map(str::to_owned).collect();
    if let Some(returned) = returned {
        fields[0] = format!("(return {})", map_symbols(&returned, &mut rename).wire());
    }
    fields[1] = map_symbols(&effects, &mut rename).wire();
    fields.join("\t")
}

fn same_observation(observed: &str, expected: &str) -> bool {
    ranked_anonymous_parameters(observed) == ranked_anonymous_parameters(expected)
}

#[test]
fn anonymous_parameter_ids_compare_by_rank_within_one_observation() {
    let symbol = |name: &str| Value::Symbol(None, name.encode_utf16().collect());
    let function = |parameter: &str, body: &str| {
        Value::List(vec![
            symbol("fn*"),
            Value::Vector(vec![symbol(parameter)]),
            Value::List(vec![symbol(body)]),
        ])
    };
    let fields = |returned: Value, effects: Vec<Value>| {
        format!(
            "(return {})\t{}\t{}\t{}",
            returned.wire(),
            Value::Vector(effects).wire(),
            wire(""),
            wire("")
        )
    };
    let pair = |a: Value, b: Value| fields(Value::Set(vec![a, b]), vec![]);
    let duplicate = |a: &str, b: &str| pair(function(a, a), function(b, b));
    let row = ACCOUNTING
        .lines()
        .find(|line| line.starts_with("sci-reader-anonymous-duplicate\t"))
        .unwrap();
    let recorded = row.split('\t').collect::<Vec<_>>()[9..13].join("\t");
    assert_eq!(recorded, duplicate("p1__9#", "p1__10#"));
    // Ids from a fresh process, after other tests in the same process, far apart.
    for (a, b) in [
        ("p1__1#", "p1__2#"),
        ("p1__20#", "p1__21#"),
        ("p1__3#", "p1__100000#"),
    ] {
        assert!(same_observation(&duplicate(a, b), &recorded), "{a} {b}");
    }
    for observed in [
        duplicate("p1__20#", "p1__20#"),
        pair(
            function("p1__20#", "p1__21#"),
            function("p1__21#", "p1__20#"),
        ),
        pair(
            function("p1__20#", "p1__20#"),
            function("p2__21#", "p2__21#"),
        ),
        pair(
            function("p1__20#", "p1__20#"),
            function("p1__21#", "p1__20#"),
        ),
        pair(function("p1__20#", "p1__20#"), function("x", "x")),
        pair(function("p1__20#", "p1__20#"), function("p1__21", "p1__21")),
        fields(Value::Set(vec![function("p1__20#", "p1__20#")]), vec![]),
        fields(
            Value::Set(vec![
                function("p1__20#", "p1__20#"),
                function("p1__21#", "p1__21#"),
            ]),
            vec![symbol("p1__20#")],
        ),
        duplicate("p1__20#", "p1__21#").replacen(&wire(""), &wire("x"), 1),
    ] {
        assert!(!same_observation(&observed, &recorded), "{observed}");
    }
    // Ids rank jointly across the value and effects and keep their read order.
    let read =
        |a: &str, b: &str| fields(Value::Vector(vec![symbol(a), symbol(b)]), vec![symbol(b)]);
    assert!(same_observation(
        &read("p1__9#", "p1__10#"),
        &read("p1__20#", "p1__21#")
    ));
    assert!(!same_observation(
        &read("p1__9#", "p1__10#"),
        &read("p1__21#", "p1__20#")
    ));
    // The parameters of one literal share its id.
    let literal = |names: [&str; 3]| fields(Value::Vector(names.map(symbol).to_vec()), vec![]);
    let recorded = literal(["p1__4#", "p2__4#", "rest__4#"]);
    assert!(same_observation(
        &literal(["p1__30#", "p2__30#", "rest__30#"]),
        &recorded
    ));
    assert!(!same_observation(
        &literal(["p1__30#", "p2__31#", "rest__30#"]),
        &recorded
    ));
    // Only the reader's own spelling is an anonymous parameter.
    for name in ["p0__4#", "p-2__4#", "rest__4#"] {
        assert!(anonymous_parameter(&name.encode_utf16().collect::<Vec<_>>()).is_some());
    }
    for name in [
        "p1__04#",
        "p01__4#",
        "q1__4#",
        "p__4#",
        "p1__4",
        "p1__#",
        "x__4__auto__",
    ] {
        assert!(anonymous_parameter(&name.encode_utf16().collect::<Vec<_>>()).is_none());
    }
}

// Syntax quote, `binding` and destructuring name their symbols
// `<name>__<id>__auto__` from separate process-wide counters. Their ids are not
// ranked: one name cannot tell which counter made it, and ranking across
// counters would depend on the process history again. A recorded observation
// must therefore not contain them.
#[test]
fn recorded_observations_contain_no_other_process_wide_ids() {
    for line in ACCOUNTING
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
    {
        let row: Vec<_> = line.split('\t').collect();
        let (returned, effects) = observation_data(&row[9..13].join("\t"))
            .unwrap_or_else(|| panic!("{}: unreadable recorded observation", row[0]));
        for value in returned.iter().chain([&effects]) {
            map_symbols(value, &mut |name| {
                let name = String::from_utf16_lossy(name);
                let process_wide = name
                    .strip_suffix("__auto__")
                    .and_then(|name| name.rsplit_once("__"))
                    .is_some_and(|(_, id)| {
                        !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit())
                    });
                assert!(
                    !process_wide,
                    "{}: {name} has a process-wide id; compare it by rank before recording it",
                    row[0]
                );
                None
            });
        }
    }
}

/// A historical SCI case with its reviewed accounting row, its disposition and the
/// evaluator observation that row requires.
struct Disposition<'a> {
    case: protocol::Case<'a>,
    row: Vec<&'a str>,
    status: &'a str,
    expected: String,
}

/// Every historical case in corpus order with its accounting row, after checking
/// that each row is complete, owned, current with its SCI row and inherited with
/// provenance, and that no accounting row is orphaned.
fn dispositions() -> Vec<Disposition<'static>> {
    let cases = protocol::cases(CORPUS).unwrap();
    let source_rows: HashMap<_, _> = CORPUS
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .map(|line| (line.split('\t').next().unwrap(), line))
        .collect();
    let mut rows = HashMap::new();
    for line in ACCOUNTING
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
    {
        let fields: Vec<_> = line.split('\t').collect();
        assert_eq!(fields.len(), 14, "accounting field count: {}", fields[0]);
        assert!(
            rows.insert(fields[0], fields).is_none(),
            "duplicate disposition"
        );
    }
    assert_eq!(
        rows.len(),
        cases.len(),
        "every historical row needs explicit accounting"
    );
    let mut provenance_ids = HashSet::new();
    for line in include_str!("../reference/fixture-origins.tsv")
        .lines()
        .filter(|line| line.starts_with("tests/fixtures/sci-outcomes.tsv\t"))
    {
        provenance_ids.extend(line.split('\t').nth(1).unwrap().split(','));
    }
    let dispositions = cases
        .into_iter()
        .map(|case| {
            assert!(
                provenance_ids.contains(case.id),
                "missing inherited SCI provenance: {}",
                case.id
            );
            let row = rows.remove(case.id).unwrap();
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
            // Select the expected canonical contract from the immutable row alone.
            // A regression back to SCI's [0 2] must fail this canonical comparison.
            let canonical = canonical_anonymous_contract(&row);
            let status = if canonical.is_some() {
                "canonical-dialect-difference"
            } else {
                status
            };
            let expected = if let Some(expected) = canonical {
                expected.to_owned()
            } else if let Some(current) = current_quoted_macro_diagnostic(&row) {
                current
            } else {
                row[9..13].join("\t")
            };
            Disposition {
                case,
                row,
                status,
                expected,
            }
        })
        .collect();
    assert!(rows.is_empty(), "orphaned evaluator dispositions");
    dispositions
}

#[test]
fn every_historical_sci_case_has_an_evaluator_disposition() {
    let mut counts = BTreeMap::<&str, usize>::new();
    let mut matching_families = BTreeMap::<String, usize>::new();
    for Disposition {
        case,
        row,
        status,
        expected,
    } in &dispositions()
    {
        let status = *status;
        *counts.entry(status).or_default() += 1;
        if status == "portable" || status == "adapted" {
            let family = case.id.split('-').take(2).collect::<Vec<_>>().join("-");
            *matching_families.entry(family).or_default() += 1;
            let projected = evaluator::Observation::portable(case)
                .unwrap_or_else(|error| panic!("{} is not portable: {error}", case.id));
            assert_eq!(
                *expected,
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
            let projected = evaluator::Observation::portable(case).map(|o| o.fields());
            assert!(
                !projected.is_ok_and(|fields| same_observation(&fields, expected)),
                "{} now agrees with the SCI projection: migrate its disposition",
                case.id
            );
        }
    }
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
    println!(
        "Evaluator accounting (gaps/retirements/canonical differences are not SCI conformance passes): {counts:?}"
    );
}

/// The corpus executes in this many tests, declared below, so they overlap under
/// nextest.
const PARTS: usize = 10;

/// Runs every `PARTS`-th historical case from `part` through the evaluator and
/// compares each observation with the one its disposition requires. Striding
/// spreads the slow families and the deadline-bound lazy-cycle probes over the
/// parts.
fn execute_dispositions(part: usize) {
    let mut failures = vec![];
    for Disposition {
        case,
        row,
        status,
        expected,
    } in dispositions().iter().skip(part).step_by(PARTS)
    {
        let actual = evaluator::run(case.id, row[8], case.profile);
        let observed = actual.as_ref().map(evaluator::Observation::fields);
        let matches = observed.as_ref().is_ok_and(|fields| {
            same_observation(fields, expected) || stack_depth_reconciliation(row, fields)
        });
        if !matches {
            let actual_text = match &observed {
                Ok(fields) => fields,
                Err(error) => *error,
            };
            let brief = |text: &str| text.chars().take(1_200).collect::<String>();
            failures.push(format!(
                "{} [{status}]: {}\nactual {}\nexpected {}\n{}",
                case.id,
                row[8],
                brief(actual_text),
                brief(expected),
                row[13]
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

row_tests!(execute_dispositions;
    historical_sci_cases_execute_their_dispositions_0(0),
    historical_sci_cases_execute_their_dispositions_1(1),
    historical_sci_cases_execute_their_dispositions_2(2),
    historical_sci_cases_execute_their_dispositions_3(3),
    historical_sci_cases_execute_their_dispositions_4(4),
    historical_sci_cases_execute_their_dispositions_5(5),
    historical_sci_cases_execute_their_dispositions_6(6),
    historical_sci_cases_execute_their_dispositions_7(7),
    historical_sci_cases_execute_their_dispositions_8(8),
    historical_sci_cases_execute_their_dispositions_9(9),
);

#[test]
fn cljs_projection_keeps_numeric_bits_and_explicitly_rejects_jvm_only_numbers() {
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
