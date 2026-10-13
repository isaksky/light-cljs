//! Atom and volatile operations, constructors and watch protocol returns.

/// Complete sources in both compiler phases and both host lifecycles, each run in
/// QuickJS and Node (see `tests/support/phase_oracle.rs`).
mod phase_oracles {
    use crate::phase_oracle::{Case, run};

    #[test]
    fn atoms_and_volatiles() {
        run(&[
            Case::expression(
                "atoms-and-volatile",
                include_str!("../fixtures/phase-oracles/state/atoms-and-volatile.cljs"),
            ),
            Case::observed(
                "watch-protocol-returns",
                include_str!("../fixtures/phase-oracles/state/watch-protocol-returns.cljs"),
                r#"{"id":"watch-protocol-returns","outcome":{"status":"returned","value":{"$vector":[true,true,true,true]}},"trace":[],"stdout":""}"#,
            ),
            Case::observed(
                "constructors",
                include_str!("../fixtures/phase-oracles/state/constructors.cljs"),
                r#"{"id":"constructors","outcome":{"status":"returned","value":{"$vector":[1,2,3,4,5,true,false,true,false,false,false,true,false]}},"trace":[],"stdout":""}"#,
            ),
            Case::observed_during_expansion(
                "constructors-during-expansion",
                include_str!("../fixtures/phase-oracles/state/constructors.cljs"),
                r#"{"id":"constructors-during-expansion","outcome":{"status":"returned","value":{"$vector":[1,2,3,4,5,true,false,true,false,false,false,true,false]}},"trace":[],"stdout":""}"#,
            ),
        ]);
    }
}
