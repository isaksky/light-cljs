//! Var predicates, resolution, defonce, declarations and exists? checks.

/// Complete sources in both compiler phases and both host lifecycles, each run in
/// QuickJS and Node (see `tests/support/phase_oracle.rs`).
mod phase_oracles {
    use crate::phase_oracle::{Case, run};

    #[test]
    fn definition_test_bodies() {
        run(&[
            Case::expression(
                "var-predicate",
                include_str!("../fixtures/phase-oracles/definitions/var-predicate.cljs"),
            ),
            Case::expression(
                "resolve",
                include_str!("../fixtures/phase-oracles/definitions/resolve.cljs"),
            ),
            Case::expression(
                "defonce-cljs-3333",
                include_str!("../fixtures/phase-oracles/definitions/defonce-cljs-3333.cljs"),
            ),
        ]);
    }

    #[test]
    fn declarations() {
        run(&[
            Case::observed_during_expansion(
                "declare-forward",
                include_str!("../fixtures/phase-oracles/definitions/declare-forward.cljs"),
                r#"{"id":"declare-forward","outcome":{"status":"returned","value":true},"trace":[],"stdout":""}"#,
            ),
            Case::observed_during_expansion(
                "declare-after",
                include_str!("../fixtures/phase-oracles/definitions/declare-after.cljs"),
                r#"{"id":"declare-after","outcome":{"status":"returned","value":true},"trace":[],"stdout":""}"#,
            ),
            Case::observed_during_expansion(
                "exists",
                include_str!("../fixtures/phase-oracles/definitions/exists.cljs"),
                r#"{"id":"exists","outcome":{"status":"returned","value":true},"trace":[],"stdout":""}"#,
            ),
            Case::observed_during_expansion(
                "declare-result",
                include_str!("../fixtures/phase-oracles/definitions/declare-result.cljs"),
                r#"{"id":"declare-result","outcome":{"status":"returned","value":true},"trace":[],"stdout":""}"#,
            ),
        ]);
    }
}
