//! Validate the provenance contract without requiring the research checkout.
use std::collections::HashSet;

const CASE_FILES: [(&str, &str, usize); 10] = [
    (
        "tests/fixtures/compiler-public-protocols.tsv",
        include_str!("fixtures/compiler-public-protocols.tsv"),
        3,
    ),
    (
        "tests/fixtures/compiler-public-protocol-bodies.tsv",
        include_str!("fixtures/compiler-public-protocol-bodies.tsv"),
        4,
    ),
    (
        "tests/fixtures/compiler-transients.tsv",
        include_str!("fixtures/compiler-transients.tsv"),
        4,
    ),
    (
        "tests/fixtures/compiler-language.tsv",
        include_str!("fixtures/compiler-language.tsv"),
        6,
    ),
    (
        "tests/fixtures/sci-outcomes.tsv",
        include_str!("fixtures/sci-outcomes.tsv"),
        7,
    ),
    (
        "tests/fixtures/analysis-boundaries.tsv",
        include_str!("fixtures/analysis-boundaries.tsv"),
        9,
    ),
    (
        "tests/fixtures/execution-literals.tsv",
        include_str!("fixtures/execution-literals.tsv"),
        4,
    ),
    (
        "tests/fixtures/execution.tsv",
        include_str!("fixtures/execution.tsv"),
        4,
    ),
    (
        "tests/fixtures/execution-outcomes.tsv",
        include_str!("fixtures/execution-outcomes.tsv"),
        5,
    ),
    (
        "tests/reference/lexical-reference-limits.tsv",
        include_str!("reference/lexical-reference-limits.tsv"),
        4,
    ),
];
const ORIGINS: &str = include_str!("reference/fixture-origins.tsv");

fn rows(text: &str) -> impl Iterator<Item = &str> {
    text.lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
}

fn contains_rust_test(source: &str, id: &str) -> bool {
    let signature = format!("fn {id}()");
    source
        .lines()
        .zip(source.lines().skip(1))
        .any(|(attribute, declaration)| {
            attribute.trim() == "#[test]" && declaration.trim_start().starts_with(&signature)
        })
}

fn validate_origin(row: &str) -> Result<[&str; 9], &'static str> {
    let fields: [&str; 9] = row
        .split('\t')
        .collect::<Vec<_>>()
        .try_into()
        .map_err(|_| "expected nine provenance columns")?;
    let [_, ids, origin, repository, commit, path, anchor, rationale, behavior] = fields;
    if fields.iter().any(|field| field.trim().is_empty())
        || ids.split(',').any(|id| id.is_empty())
        || rationale == "-"
        || behavior == "-"
    {
        return Err("missing fixture identity, rationale, or behavior");
    }
    match origin {
        "hand-authored" => {
            if [repository, commit, path, anchor] != ["-"; 4] {
                return Err("hand-authored fixtures must not claim an upstream source");
            }
        }
        "imported" | "adapted" | "generated" => {
            if !repository.starts_with("https://")
                || commit.len() != 40
                || !commit.bytes().all(|b| b.is_ascii_hexdigit())
                || path == "-"
                || path.starts_with('/')
                || path.split('/').any(|part| part.is_empty() || part == "..")
                || anchor == "-"
            {
                return Err(
                    "derived fixtures require a repository, full commit, relative source path, and anchor",
                );
            }
        }
        _ => return Err("unknown origin category"),
    }
    Ok(fields)
}

#[test]
fn fixture_ids_are_unique_and_have_complete_provenance() {
    let mut cases = HashSet::new();
    for (path, text, columns) in CASE_FILES {
        for row in rows(text) {
            let fields: Vec<_> = row.split('\t').collect();
            assert_eq!(fields.len(), columns, "invalid fixture: {row}");
            if path == "tests/fixtures/analysis-boundaries.tsv" {
                assert!(
                    matches!(fields[1], "expr" | "ns" | "statement" | "cljc"),
                    "invalid reference context: {row}"
                );
                assert!(!fields[3].is_empty(), "missing diagnostic span: {row}");
                for classification in &fields[7..] {
                    assert!(
                        matches!(
                            *classification,
                            "reader-error"
                                | "analysis-error"
                                | "emit-error"
                                | "javascript-error"
                                | "warning"
                                | "accepted"
                        ),
                        "unverified classification: {row}"
                    );
                }
            }
            let id = fields[0];
            assert!(
                !id.is_empty()
                    && id
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'),
                "invalid stable fixture ID: {id}"
            );
            assert!(cases.insert((path, id)), "duplicate fixture ID: {id}");
        }
    }
    assert!(!cases.is_empty());
    assert_eq!(
        cases.iter().map(|(_, id)| id).collect::<HashSet<_>>().len(),
        cases.len(),
        "fixture IDs must be unique across fixture files"
    );

    let mut documented = HashSet::new();
    let mut covered_cases = HashSet::new();
    for row in rows(ORIGINS) {
        let [local_path, ids, ..] = validate_origin(row).unwrap();
        for id in ids.split(',') {
            assert!(
                documented.insert((local_path, id)),
                "duplicate provenance for {local_path}::{id}"
            );
            match local_path {
                "tests/fixtures/compiler-public-protocols.tsv"
                | "tests/fixtures/compiler-public-protocol-bodies.tsv"
                | "tests/fixtures/sci-outcomes.tsv"
                | "tests/fixtures/compiler-language.tsv"
                | "tests/fixtures/compiler-transients.tsv"
                | "tests/fixtures/execution.tsv"
                | "tests/fixtures/execution-outcomes.tsv"
                | "tests/fixtures/execution-literals.tsv"
                | "tests/fixtures/analysis-boundaries.tsv"
                | "tests/reference/lexical-reference-limits.tsv" => {
                    assert!(
                        cases.contains(&(local_path, id)),
                        "unknown fixture ID: {id}"
                    );
                    covered_cases.insert((local_path, id));
                }
                "tests/reference/compiler-language/namespace-cases.jsonl" => assert!(
                    include_str!("reference/compiler-language/namespace-cases.jsonl")
                        .contains(&format!("\"id\":\"{id}\"")),
                    "unknown namespace witness: {id}"
                ),
                "tests/reference/compiler-language/diagnostic-cases.jsonl" => assert!(
                    include_str!("reference/compiler-language/diagnostic-cases.jsonl")
                        .contains(&format!("\"id\":\"{id}\"")),
                    "unknown diagnostic witness: {id}"
                ),
                "tests/reference/compiler-language/warning-cases.jsonl" => assert!(
                    include_str!("reference/compiler-language/warning-cases.jsonl")
                        .contains(&format!("\"id\":\"{id}\"")),
                    "unknown warning witness: {id}"
                ),
                "tests/reference/compiler-language/boundary-support.jsonl" => assert!(
                    include_str!("reference/compiler-language/boundary-support.jsonl")
                        .contains(&format!("\"id\":\"{id}\"")),
                    "unknown supplemental boundary witness: {id}"
                ),
                "tests/execution.rs" => assert!(
                    include_str!("execution.rs").contains(&format!("\nfn {id}()")),
                    "unknown execution test: {id}"
                ),
                "tests/compiler_macros.rs" => assert!(
                    contains_rust_test(include_str!("compiler_macros.rs"), id),
                    "unknown compiler macro test: {id}"
                ),
                "tests/compiler_reader.rs" => assert!(
                    contains_rust_test(include_str!("compiler_reader.rs"), id),
                    "unknown compiler reader test: {id}"
                ),
                "tests/compiler_public_protocols.rs" => assert!(
                    contains_rust_test(include_str!("compiler_public_protocols.rs"), id),
                    "unknown public protocol test: {id}"
                ),
                "tests/compiler_core_var_metadata.rs" => assert!(
                    contains_rust_test(include_str!("compiler_core_var_metadata.rs"), id),
                    "unknown core metadata test: {id}"
                ),
                "tests/fixtures/compiler-core-var-metadata.cljs" => assert!(
                    include_str!("fixtures/compiler-core-var-metadata.cljs")
                        .contains(&format!("(defn {id} ")),
                    "unknown core metadata contract: {id}"
                ),
                "tests/fixtures/reader-coverage/cljs/syntax_quote_test.cljs" => assert!(
                    include_str!("fixtures/reader-coverage/cljs/syntax_quote_test.cljs")
                        .contains(&format!("(deftest {id}")),
                    "unknown syntax quote body: {id}"
                ),
                "tests/fixtures/reader-coverage/cljs/tagged_literals_test.cljs" => assert!(
                    include_str!("fixtures/reader-coverage/cljs/tagged_literals_test.cljs")
                        .contains(&format!("(deftest {id}")),
                    "unknown tagged literal body: {id}"
                ),
                "src/compiler_bootstrap.rs" => assert!(
                    contains_rust_test(include_str!("../src/compiler_bootstrap.rs"), id),
                    "unknown compiler bootstrap test: {id}"
                ),
                "tests/namespaces.rs" => assert!(
                    include_str!("namespaces.rs").contains(&format!("#[test]\nfn {id}()")),
                    "unknown namespace test: {id}"
                ),
                "tests/analyzer.rs" => assert!(
                    include_str!("analyzer.rs").contains(&format!("#[test]\nfn {id}()")),
                    "unknown analyzer test: {id}"
                ),
                "tests/eval_cli.rs" => assert!(
                    include_str!("eval_cli.rs").contains(&format!("#[test]\nfn {id}()")),
                    "unknown native CLI test: {id}"
                ),
                _ => panic!("register a local fixture source before using it: {local_path}"),
            }
        }
    }
    assert_eq!(covered_cases, cases, "every fixture needs provenance");
}

#[test]
fn derived_provenance_requires_pinned_traceable_sources() {
    let row = rows(ORIGINS)
        .find(|row| row.contains("\tadapted\t"))
        .unwrap();
    for category in ["imported", "adapted", "generated"] {
        let valid = row.replace("\tadapted\t", &format!("\t{category}\t"));
        assert!(validate_origin(&valid).is_ok());
        for column in [1, 3, 4, 5, 6, 7, 8] {
            let mut fields: Vec<_> = valid.split('\t').collect();
            fields[column] = "";
            assert!(validate_origin(&fields.join("\t")).is_err());
        }
        let mut fields: Vec<_> = valid.split('\t').collect();
        fields[4] = "a5e94c9";
        assert!(validate_origin(&fields.join("\t")).is_err());
    }
    assert!(validate_origin(&row.replace("\tadapted\t", "\thand-authored\t")).is_err());
    assert!(validate_origin(&row.replace("\tadapted\t", "\tunknown\t")).is_err());
}
