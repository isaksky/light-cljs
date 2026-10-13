//! Conditional bindings, constants, exceptions and the empty and/or forms.

/// Complete sources in both compiler phases and both host lifecycles, each run in
/// QuickJS and Node (see `tests/support/phase_oracle.rs`).
mod phase_oracles {
    use crate::phase_oracle::{Case, run};

    #[test]
    fn conditional_bindings() {
        run(&[
            Case::observed_during_expansion(
                "nil-bindings",
                include_str!("../fixtures/phase-oracles/control/nil-bindings.cljs"),
                r#"{"id":"nil-bindings","outcome":{"status":"returned","value":true},"trace":[],"stdout":""}"#,
            ),
            Case::observed_during_expansion(
                "binding-scope",
                include_str!("../fixtures/phase-oracles/control/binding-scope.cljs"),
                r#"{"id":"binding-scope","outcome":{"status":"returned","value":true},"trace":[],"stdout":""}"#,
            ),
            Case::observed_during_expansion(
                "binding-once",
                include_str!("../fixtures/phase-oracles/control/binding-once.cljs"),
                r#"{"id":"binding-once","outcome":{"status":"returned","value":true},"trace":[2,3],"stdout":""}"#,
            ),
            Case::observed_during_expansion(
                "when-first",
                include_str!("../fixtures/phase-oracles/control/when-first.cljs"),
                r#"{"id":"when-first","outcome":{"status":"returned","value":true},"trace":[{"$vector":[2,3]}],"stdout":""}"#,
            ),
            Case::observed_during_expansion(
                "macro-contexts",
                include_str!("../fixtures/phase-oracles/control/macro-contexts.cljs"),
                r#"{"id":"macro-contexts","outcome":{"status":"returned","value":true},"trace":[],"stdout":""}"#,
            ),
        ]);
    }

    #[test]
    fn control_test_bodies() {
        run(&[
            Case::expression(
                "const-emission",
                include_str!("../fixtures/phase-oracles/control/const-emission.cljs"),
            ),
            Case::expression(
                "throwable-map",
                include_str!("../fixtures/phase-oracles/control/throwable-map.cljs"),
            ),
            Case::expression(
                "empty-and-or-cljs-3400",
                include_str!("../fixtures/phase-oracles/control/empty-and-or-cljs-3400.cljs"),
            ),
        ]);
    }
}
