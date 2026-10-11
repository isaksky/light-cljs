#[path = "support/sci_protocol.rs"]
mod protocol;
use protocol::{Category, Outcome, Phase, Value};

const FIXTURES: &str = include_str!("fixtures/sci-outcomes.tsv");

#[test]
fn committed_sci_corpus_is_typed_complete_and_pinned() {
    let cases = protocol::cases(FIXTURES).unwrap();
    assert!(cases.len() >= 39);
    assert!(FIXTURES.contains(protocol::REVISION));
    let metadata = include_str!("reference/sci/runtime.tsv");
    for expected in [
        "format\tsci-outcomes-v1",
        &format!("revision\t{}", protocol::REVISION),
        "clojure\t1.12.4",
        "java\t",
        "java-vendor\t",
        "options\t{:features #{:clj}, :read-eval false, :user [observe], :out :captured, :err :captured}",
    ] {
        assert!(
            metadata.contains(expected),
            "missing reference metadata: {expected}"
        );
    }
    for case in &cases {
        assert!(!case.source.is_empty());
        if case.profile == "bounded-number" {
            assert!(matches!(
                case.outcome,
                Outcome::Returned(Value::BigInt(_) | Value::Decimal(..))
            ));
        }
    }
    let find = |id| cases.iter().find(|case| case.id == id).unwrap();
    assert_eq!(
        find("sci-after-error").outcome,
        Outcome::Returned(Value::Int(42))
    );
    assert_eq!(
        find("sci-context-isolated").outcome,
        Outcome::Error {
            phase: Phase::Analysis,
            category: Category::UnresolvedSymbol
        }
    );
    assert_eq!(find("sci-execution-error").effects, [Value::Int(1)]);
    assert_eq!(
        find("sci-execution-error").stdout,
        "before\n".encode_utf16().collect::<Vec<_>>()
    );
    assert_eq!(
        find("sci-stdout").stderr,
        "warning".encode_utf16().collect::<Vec<_>>()
    );
    assert_eq!(
        find("sci-read-error").effects.len(),
        1,
        "completed top-level effects survive a later read error"
    );
}

#[test]
fn protocol_preserves_numeric_categories_utf16_and_collection_identity() {
    for (wire, expected) in [
        ("(int 9223372036854775807)", Value::Int(i64::MAX)),
        ("(int -9223372036854775808)", Value::Int(i64::MIN)),
        (
            "(float 8000000000000000)",
            Value::Float((-0.0f64).to_bits()),
        ),
        ("(float 7ff8000000000000)", Value::Float(f64::NAN.to_bits())),
        ("(ratio -2 3)", Value::Ratio(-2, 3)),
        (
            "(string ud8000061dc00)",
            Value::String(vec![0xd800, 0x61, 0xdc00]),
        ),
        ("(char d800)", Value::Char(0xd800)),
        ("(symbol - u0078)", Value::Symbol(None, vec![120])),
        (
            "(keyword u0061 u0078)",
            Value::Keyword(Some(vec![97]), vec![120]),
        ),
        (
            "(bigint 9223372036854775808)",
            Value::BigInt("9223372036854775808".into()),
        ),
        ("(decimal 120 2)", Value::Decimal("120".into(), 2)),
    ] {
        assert_eq!(protocol::value(wire).unwrap(), expected);
        assert_eq!(expected.wire(), wire);
    }
    assert_ne!(
        protocol::value("(int 1)"),
        protocol::value("(float 3ff0000000000000)")
    );
    assert_ne!(
        protocol::value("(list nil)"),
        protocol::value("(vector nil)")
    );
    assert_ne!(protocol::value("(seq nil)"), protocol::value("(list nil)"));
    assert_eq!(
        Value::Set(vec![Value::Int(2), Value::Int(1)]).wire(),
        "(set (int 1) (int 2))"
    );
}

#[test]
fn protocol_rejects_lossy_unknown_malformed_and_unbounded_results() {
    for invalid in [
        "null",
        "1",
        "(int 01)",
        "(int -0)",
        "(int 9223372036854775808)",
        "(float NaN)",
        "(float 7ff0000000000001)",
        "(ratio 2 4)",
        "(ratio 1 -2)",
        "(ratio 0 2)",
        "(ratio 1 1)",
        "(string ufff)",
        "(string uD800)",
        "(symbol u u0061)",
        "(keyword - u)",
        "(map (entry nil))",
        "(map (entry nil nil) (entry nil nil))",
        "(set nil nil)",
        "(set (int 2) (int 1))",
        "(object nil)",
        "(vector",
        "nil nil",
        " nil",
        "(bool  true)",
    ] {
        assert!(protocol::value(invalid).is_err(), "accepted {invalid}");
    }
    for invalid in [
        "(error emit arity)",
        "(error read arity)",
        "(error execution unknown)",
        "(return (object))",
        "(error analysis unresolved-symbol) nil",
    ] {
        assert!(protocol::outcome(invalid).is_err(), "accepted {invalid}");
    }
    assert!(protocol::value(&format!("{}nil{}", "(vector ".repeat(130), ")".repeat(130))).is_err());
    assert!(protocol::value(&format!("(vector {})", vec!["nil"; 10001].join(" "))).is_err());
    assert!(protocol::cases("").is_err());
    let row = FIXTURES
        .lines()
        .find(|line| line.starts_with("sci-scalars\t"))
        .unwrap();
    assert!(protocol::cases(&format!("{row}\n{row}")).is_err());
    assert!(protocol::cases(&row.replacen("portable", "unknown", 1)).is_err());
}

#[test]
#[ignore = "explicit SCI checkout/JVM reference job; requires LIGHT_CLJS_SCI_CHECKOUT"]
fn pinned_sci_reference_matches_committed_outcomes() {
    let checkout = std::env::var_os("LIGHT_CLJS_SCI_CHECKOUT")
        .expect("Set LIGHT_CLJS_SCI_CHECKOUT to the clean pinned SCI source checkout");
    let output =
        std::process::Command::new(std::env::var_os("PYTHON").unwrap_or_else(|| "python3".into()))
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .args([
                "tests/reference/sci/regenerate.py",
                "--evaluator",
                "--checkout",
            ])
            .arg(checkout)
            .output()
            .expect("explicit SCI reference requires Python 3");
    assert!(
        output.status.success(),
        "SCI reference failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    eprintln!("{}", String::from_utf8_lossy(&output.stdout));
}
