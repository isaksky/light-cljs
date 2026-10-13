#[path = "../benches/support/variants.rs"]
mod variants;

#[path = "../benches/support/subset.rs"]
mod subset;
use subset::{Token, Tokenizer};
use variants::{CompactTokenizer, MatchTokenizer, SourceTooLarge};

#[path = "../benches/support/levels.rs"]
mod levels;
#[path = "support/ratchet_fixtures.rs"]
#[allow(dead_code)]
mod ratchet_fixtures;

/// `cargo bench --bench compiler -- --levels` at its smallest: two fixtures, one
/// timed round, tiny steady-state inputs. Every sample is checked as in a real run.
#[test]
fn level_benchmark_harness_checks_and_records_every_level() {
    let output =
        std::env::temp_dir().join(format!("light-cljs-levels-smoke-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&output);
    let fixtures: Vec<_> = ratchet_fixtures::FIXTURES
        .iter()
        .filter(|fixture| ["seq-pipeline", "rest-fn"].contains(&fixture.name))
        .map(|fixture| levels::Startup {
            name: fixture.name,
            source: fixture.source,
            printed: fixture.printed,
        })
        .collect();
    // `--only` takes fixture and workload names.
    assert!(levels::is_workload("event-maps") && !levels::is_workload("rest-fn"));
    let mut config = levels::Config::new(output.clone());
    config.samples = 1;
    config.warmups = 0;
    config.input = Some(10);
    config.warmup_calls = 1;
    config.calls = 2;
    levels::run(&config, &fixtures).unwrap();
    let samples = std::fs::read_to_string(output.join("samples.csv")).unwrap();
    // Startup: 2 fixtures x 3 levels x (4 Node + 1 QuickJS measures). Steady state:
    // 8 workloads x 3 levels x 2 engines.
    assert_eq!(samples.lines().count(), 1 + 30 + 48, "{samples}");
    for row in [
        "node,parse+execute,seq-pipeline,advanced,0,",
        "quickjs,parse+execute,rest-fn,none,0,",
        "node,call,record-protocol,prepare,0,",
        "quickjs,call,seq-pipeline,advanced,0,",
        "quickjs,call,keyword-lookups,none,0,",
        "node,call,constant-literals,advanced,0,",
        "quickjs,call,constant-keys,advanced,0,",
        "quickjs,call,multi-arity,advanced,0,",
        "node,call,event-maps,advanced,0,",
        "quickjs,call,closed-params,advanced,0,",
    ] {
        assert!(samples.contains(row), "missing {row}:\n{samples}");
    }
    let summary = std::fs::read_to_string(output.join("summary.md")).unwrap();
    assert!(summary.contains("| rest-fn | prepare |"), "{summary}");
    assert!(
        summary.contains("| record-protocol | 10 | none |"),
        "{summary}"
    );
    assert!(
        summary.contains("Not measured; pass `--upstream"),
        "{summary}"
    );
    std::fs::remove_dir_all(output).unwrap();
}

fn check(source: &str) {
    let expected: Vec<_> = Tokenizer::new(source).collect();
    let mut baseline = MatchTokenizer::new(source);
    let actual: Vec<_> = baseline.by_ref().collect();
    assert_eq!(actual, expected);
    let mut compact = CompactTokenizer::new(source).unwrap();
    let actual: Vec<_> = compact
        .by_ref()
        .map(|token| Token {
            kind: token.kind,
            span: token.span.start as usize..token.span.end as usize,
        })
        .collect();
    assert_eq!(actual, expected);
    for _ in 0..3 {
        assert_eq!(baseline.next(), None);
        assert_eq!(compact.next(), None);
    }
}

#[test]
fn experimental_scanners_agree_with_library() {
    check("");
    check("(+ 1 2)");
    // All ASCII bytes exercise the entire delimiter classification domain.
    // Non-ASCII byte values are also exercised through valid UTF-8 encodings.
    for ch in (0..=0x10ffff).filter_map(char::from_u32) {
        let source = format!("(x{ch}λ),{ch} {ch}");
        check(&source);
    }
}

#[test]
fn compact_source_limit_is_checked_before_narrowing() {
    assert_eq!(CompactTokenizer::validate_len(0), Ok(()));
    assert_eq!(CompactTokenizer::validate_len(u32::MAX as usize), Ok(()));
    if let Ok(too_large) = usize::try_from(u64::from(u32::MAX) + 1) {
        assert_eq!(
            CompactTokenizer::validate_len(too_large),
            Err(SourceTooLarge)
        );
        assert_eq!(
            CompactTokenizer::validate_len(usize::MAX),
            Err(SourceTooLarge)
        );
    }
}
