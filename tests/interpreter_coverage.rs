//! Explicit supported surface inventory, checked against the actual runtime.
use light_cljs::interpreter::{Context, Name, ValueView};
use std::collections::{BTreeMap, BTreeSet};
#[path = "support/sci_protocol.rs"]
#[allow(dead_code)]
mod protocol;

#[test]
fn every_runtime_api_has_reviewed_executable_evidence() {
    let cases = protocol::cases(include_str!("fixtures/sci-outcomes.tsv")).unwrap();
    let mut mapped = BTreeMap::new();
    for row in include_str!("reference/sci/supported-apis.tsv")
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
    {
        let fields: Vec<_> = row.split('\t').collect();
        assert_eq!(fields.len(), 5, "{row}");
        let [api, kind, disposition, evidence, note] = fields[..] else {
            unreachable!()
        };
        assert!(
            mapped.insert(api.to_owned(), kind).is_none(),
            "duplicate {api}"
        );
        assert!(!note.is_empty(), "{api}");
        match disposition {
            "sci-exact" | "via-expansion" => {
                let case = cases
                    .iter()
                    .find(|c| c.id == evidence)
                    .unwrap_or_else(|| panic!("unknown coverage evidence {api}: {evidence}"));
                assert!(
                    matches!(case.outcome, protocol::Outcome::Returned(_)),
                    "coverage must reach a return: {api}"
                );
                assert!(
                    !include_str!("reference/sci/native-dispositions.tsv")
                        .lines()
                        .any(|l| l.starts_with(&format!("{evidence}\t"))),
                    "{api} incorrectly claims exact agreement"
                );
            }
            "native-only" | "unsupported-explicit" => {
                let (path, test) = evidence
                    .split_once("::")
                    .expect("native coverage needs path::test");
                let source = std::fs::read_to_string(
                    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(path),
                )
                .unwrap();
                assert!(
                    source.contains(&format!("#[test]\nfn {test}(")),
                    "unknown native coverage {api}: {evidence}"
                );
                assert!(source.contains("#[test]"), "{evidence}");
            }
            _ => panic!("unknown coverage disposition {row}"),
        }
    }
    let mut c = Context::new();
    // These lazy builtins are not included by the namespace map initializer.
    for name in ["*file*", "unquote"] {
        c.resolve_var(&Name::new(Some("clojure.core"), name))
            .unwrap()
            .unwrap();
    }
    let mut actual = BTreeSet::new();
    for namespace in [
        "clojure.core",
        "clojure.string",
        "clojure.set",
        "clojure.walk",
        "clojure.edn",
    ] {
        let values = c
            .eval_string(&format!("(vec (keys (ns-interns '{namespace})))"))
            .unwrap();
        for name in values.elements().unwrap() {
            let ValueView::Symbol(name) = name.inspect() else {
                panic!("invalid namespace key")
            };
            actual.insert(format!(
                "{namespace}/{}",
                String::from_utf16(name.name()).unwrap()
            ));
        }
    }
    // Analyzer primitives/clauses have no ordinary callable Var.
    for name in [
        "def", "var", "quote", "if", "do", "let*", "loop*", "recur", "letfn*", "throw", "try",
        "set!", "case*", "fn*", "declare", "defonce",
    ] {
        actual.insert(format!("clojure.core/{name}"));
    }
    assert_eq!(
        actual,
        mapped.keys().cloned().collect(),
        "runtime surface changed without a coverage disposition"
    );
    for (api, kind) in mapped {
        assert!(
            matches!(
                kind,
                "function"
                    | "macro"
                    | "variable"
                    | "helper"
                    | "primitive"
                    | "special"
                    | "special-var"
                    | "analyzer"
                    | "special-form"
                    | "analyzer-form"
                    | "sentinel"
            ),
            "{api}: {kind}"
        );
    }
}

#[test]
fn unsupported_families_fail_explicitly() {
    use light_cljs::interpreter::{EvalErrorKind as E, EvalPhase, NativeError};
    for row in include_str!("reference/sci/unsupported-families.tsv")
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
    {
        let fields: Vec<_> = row.split('\t').collect();
        let [family, source, phase, category, reason] = fields[..] else {
            panic!("invalid disposition {row}")
        };
        assert!(!reason.is_empty());
        let error = Context::new().eval_string(source).unwrap_err();
        let actual_phase = match error.phase {
            EvalPhase::Read => "read",
            EvalPhase::Analysis => "analysis",
            EvalPhase::Execution => "execution",
            EvalPhase::Macroexpansion => "macroexpansion",
        };
        let actual = match error.kind {
            E::UnresolvedSymbol(_) => "unresolved",
            E::UnsupportedForm(_) => "unsupported",
            E::InvalidForm(_) => "invalid-form",
            E::Read(_) => "reader",
            E::Native {
                error: NativeError::Arity { .. },
                ..
            } => "arity",
            E::Native {
                error: NativeError::UnsupportedNumber(_),
                ..
            } => "unsupported-number",
            _ => panic!("unexpected {family}: {error}"),
        };
        assert_eq!(
            (actual_phase, actual),
            (phase, category),
            "{family}: {error}"
        );
    }
}
