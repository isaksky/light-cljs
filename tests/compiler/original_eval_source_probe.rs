//! Diagnostic only: unchanged original eval expressions through installed EvalSession.
//! No coverage registration and no replacement runtime evaluator.
use light_cljs::{EvalOptions, EvalSession, source_provider::SourceInput};
use std::{fs, io::Write};

#[test]
#[ignore = "explicit original eval source diagnostic; no semantic coverage credit"]
fn whole_original_eval_body_uses_installed_application_service() {
    let source = fs::read_to_string(std::env::var("LIGHT_ORIGINAL_EVAL_SOURCE").unwrap()).unwrap();
    let original = include_str!("../../research/clojurescript/src/test/cljs/cljs/eval_test.cljs");
    let retained = original.split_once(";;; This test namespace").unwrap().1;
    assert!(
        source.contains(retained),
        "complete original declarations/body must be retained"
    );
    let mut output =
        fs::File::create(std::env::var("LIGHT_ORIGINAL_EVAL_OUTPUT").unwrap()).unwrap();
    for reused in [false, true] {
        let mut session = EvalSession::new(EvalOptions {
            namespace: "cljs.eval-test".into(),
            ..Default::default()
        })
        .unwrap();
        if reused {
            let warmup = session
                .eval_string("(def warmup-root (atom 0)) (defmacro warmup [] true) (warmup)")
                .unwrap();
            session.release(warmup.value).unwrap();
        }
        let observation = match session.eval_source(&SourceInput::new(
            "original-eval-source-probe.cljs",
            &source,
        )) {
            Ok(result) => {
                let text = session.print(result.value).unwrap();
                session.release(result.value).unwrap();
                text
            }
            Err(error) => format!("top-level-error: {}", error),
        };
        writeln!(
            output,
            "{}\t{}",
            if reused {
                "application-reused"
            } else {
                "application-fresh"
            },
            observation
                .as_bytes()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        )
        .unwrap();
        output.flush().unwrap();
    }
}
