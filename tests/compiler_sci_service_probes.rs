//! Distinguish hosted compiler services from standalone emitted-JS evidence.
//! These original SCI operations have independently observed matching return
//! values in both pinned canonical compilers. This diagnostic prints macro
//! failures explicitly; it never grants standalone or macro corpus coverage.
use light_cljs::{EvalOptions, EvalSession};

#[test]
#[ignore = "explicit SCI service/phase diagnostic; records macro failures, no gate coverage"]
fn audit_original_sci_macroexpand_phase_services() {
    let mut observations = String::new();
    for (id, source, expected) in [
        (
            "sci-binding-macroexpand-let",
            "(first (macroexpand-1 '(let [[x] [1]] x)))",
            "let*",
        ),
        (
            "sci-binding-macroexpand-fn",
            "(first (macroexpand-1 '(fn [[x]] x)))",
            "fn*",
        ),
        (
            "sci-binding-macroexpand-defn",
            "(first (macroexpand-1 '(defn f [x] x)))",
            "def",
        ),
        (
            "sci-binding-macroexpand-letfn",
            "(seq? (macroexpand-1 '(letfn [(f [x] x)] (f 1))))",
            "true",
        ),
    ] {
        let original = include_str!("fixtures/sci-outcomes.tsv")
            .lines()
            .find(|line| line.split('\t').next() == Some(id))
            .unwrap();
        assert_eq!(original.split('\t').nth(2), Some(source));
        let mut session = EvalSession::new(EvalOptions {
            namespace: "user".into(),
            ..Default::default()
        })
        .unwrap();
        let result = session.eval_string(source).unwrap();
        let application = session.print(result.value).unwrap();
        session.release(result.value).unwrap();
        assert_eq!(application, expected, "hosted application: {id}");
        let macro_source =
            format!("(defmacro service-probe [] (list 'quote (do {source}))) (service-probe)");
        let macro_result = match session.eval_string(&macro_source) {
            Ok(result) => {
                let printed = session.print(result.value).unwrap();
                session.release(result.value).unwrap();
                format!("macro-returned={printed}")
            }
            Err(error) => format!("macro-error={}", error.message.replace(['\n', '\t'], " ")),
        };
        observations.push_str(&format!(
            "{id}\tapplication={application}\t{macro_result}\n"
        ));
    }
    print!("{observations}");
    if let Some(path) = std::env::var_os("LIGHT_CLJS_SCI_PHASE_PROBE_OUTPUT") {
        std::fs::write(path, observations).unwrap();
    }
}
