#[path = "support/generated_native.rs"]
mod generated;
#[path = "support/sci_native.rs"]
#[allow(dead_code)]
mod native;
#[path = "support/sci_protocol.rs"]
#[allow(dead_code)]
mod protocol;
use generated::{Program, SEEDS};

#[derive(Debug)]
struct Failure {
    variant: usize,
    kind: String,
    detail: String,
}
fn compare(program: &Program) -> Result<(), Failure> {
    let sources = program.sources();
    let mut baseline = None;
    for (variant, (id, source)) in program.ids().iter().zip(&sources).enumerate() {
        assert!(source.len() < 8192);
        let actual = native::run(source, "portable").map_err(|detail| Failure {
            variant,
            kind: "transport".into(),
            detail: format!("{id}: {detail}\n{source}"),
        })?;
        if !matches!(actual.outcome, protocol::Outcome::Returned(_)) {
            return Err(Failure {
                variant,
                kind: format!("{:?}", actual.outcome),
                detail: format!("{id} did not return: {actual:?}\n{source}"),
            });
        }
        if let Some(ref expected) = baseline {
            if expected != &actual {
                return Err(Failure {
                    variant,
                    kind: "metamorphic".into(),
                    detail: format!("{id}: {actual:?} != {expected:?}\n{source}"),
                });
            }
        } else {
            baseline = Some(actual);
        }
    }
    Ok(())
}
#[test]
fn generated_programs_are_bounded_replayable_and_match_committed_sci() {
    let fixtures = protocol::cases(include_str!("fixtures/sci-outcomes.tsv")).unwrap();
    let seeds = match std::env::var("LIGHT_CLJS_NATIVE_SEED") {
        Ok(value) => vec![
            value
                .parse::<u64>()
                .expect("LIGHT_CLJS_NATIVE_SEED must be decimal u64"),
        ],
        Err(std::env::VarError::NotPresent) => SEEDS.to_vec(),
        Err(e) => panic!("{e}"),
    };
    for seed in seeds {
        let program = Program::new(seed);
        // Explicit opt-in export keeps regeneration out of ordinary tests.
        if let Some(path) = std::env::var_os("LIGHT_CLJS_NATIVE_EXPORT") {
            assert!(
                std::env::var_os("LIGHT_CLJS_NATIVE_SEED").is_some(),
                "export needs one seed"
            );
            let text: String = program
                .ids()
                .iter()
                .zip(program.sources())
                .map(|(id, source)| {
                    format!(
                        "{id}\tportable\t{source}\t(return nil)\t(vector)\t(string u)\t(string u)\n"
                    )
                })
                .collect();
            std::fs::write(path, text).unwrap();
            continue;
        }
        if let Err(error) = compare(&program) {
            let reduced = program.reduce(|p| {
                compare(p).is_err_and(|failure| {
                    failure.variant == error.variant && failure.kind == error.kind
                })
            });
            panic!(
                "native generator v1 seed={seed}; replay: LIGHT_CLJS_NATIVE_SEED={seed} cargo test --test interpreter_generated\n{}\nfocused reduction:\n{}",
                error.detail,
                reduced.sources().join("\n")
            );
        }
        if SEEDS.contains(&seed) {
            for (id, source) in program.ids().iter().zip(program.sources()) {
                let case = fixtures
                    .iter()
                    .find(|c| c.id == id)
                    .unwrap_or_else(|| panic!("missing committed generated case {id}"));
                assert_eq!(source, case.source, "generator drift: {id}");
                assert_eq!(
                    native::run(&source, "portable").unwrap(),
                    native::Observation::expected(case),
                    "seed={seed} {id}: {source}"
                );
            }
        }
    }
}
#[test]
fn focused_reduction_requires_reproducing_failure_and_terminates() {
    let program = Program::new(42);
    let unchanged = program.reduce(|_| false);
    assert_eq!(program.sources(), unchanged.sources());
    let mut attempts = 0;
    let smaller = program.reduce(|_| {
        attempts += 1;
        true
    });
    assert!(attempts <= 32);
    assert!(smaller.sources()[0].len() < program.sources()[0].len());
    compare(&smaller).unwrap();
}

#[test]
fn replay_exported_or_reduced_sci_corpus() {
    let Some(path) = std::env::var_os("LIGHT_CLJS_NATIVE_REPLAY") else {
        return;
    };
    let text = std::fs::read_to_string(path).expect("required native replay corpus");
    assert!(text.len() <= 512 * 1024, "replay exceeds 512 KiB");
    let cases = protocol::cases(&text).unwrap();
    assert!(cases.len() <= 48, "focused replay is limited to 48 cases");
    for case in cases {
        assert_eq!(
            native::run(case.source, case.profile).unwrap(),
            native::Observation::expected(&case),
            "replay {}: {}",
            case.id,
            case.source
        );
    }
}
